use crate::config::LinkPattern;
use crate::pattern::compiler::{self, PatternNode};
use crate::pattern::{CompileMatcher, Matcher};
use anyhow::Result;
use regex::RegexSet;
use std::rc::Rc;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone)]
enum LinkPatternLeaf {
    /// Regex applied to entire URL.
    Regex(String),
    /// Restricted regex applied only to hostname.
    Domain(String),
}

impl From<&LinkPattern> for Rc<PatternNode<LinkPatternLeaf>> {
    fn from(p: &LinkPattern) -> Rc<PatternNode<LinkPatternLeaf>> {
        Rc::new(match p {
            LinkPattern::Word { word } => PatternNode::Leaf {
                leaf: LinkPatternLeaf::Regex(format!(
                    r"(?i:\b{word}\b)",
                    word = regex::escape(&word)
                )),
            },
            LinkPattern::Regex { regex } => PatternNode::Leaf {
                leaf: LinkPatternLeaf::Regex(regex.clone()),
            },
            LinkPattern::Domain { domain } => PatternNode::Leaf {
                leaf: LinkPatternLeaf::Domain(format!(
                    r"(?i:\b{domain}$)",
                    domain = regex::escape(&domain)
                )),
            },
            LinkPattern::Any { any } => PatternNode::Any {
                children: any.into_iter().map(|x| Self::from(x)).collect(),
            },
            LinkPattern::All { all } => PatternNode::All {
                children: all.into_iter().map(|x| Self::from(x)).collect(),
            },
            LinkPattern::Not { not } => PatternNode::Not {
                child: Self::from(not.as_ref()),
            },
        })
    }
}

#[derive(Debug, Clone)]
enum LinkMatcherInner {
    AllRegexes(RegexSet),
    AnyRegexes(RegexSet),
    AllDomains(RegexSet),
    AnyDomains(RegexSet),
    Any(Vec<Self>),
    All(Vec<Self>),
    Not(Box<Self>),
}

impl LinkMatcherInner {
    pub fn from(node: Rc<PatternNode<LinkPatternLeaf>>) -> Result<Self> {
        Ok(match node.as_ref() {
            PatternNode::Leaf {
                leaf: LinkPatternLeaf::Regex(regex),
            } => Self::AnyRegexes(RegexSet::new(&[regex])?),
            PatternNode::Leaf {
                leaf: LinkPatternLeaf::Domain(domain),
            } => Self::AnyDomains(RegexSet::new(&[domain])?),
            PatternNode::Any { children } => {
                let regexes = children
                    .iter()
                    .flat_map(|child| match child.as_ref() {
                        PatternNode::Leaf {
                            leaf: LinkPatternLeaf::Regex(regex),
                        } => Some(regex),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let domains = children
                    .iter()
                    .flat_map(|child| match child.as_ref() {
                        PatternNode::Leaf {
                            leaf: LinkPatternLeaf::Domain(domain),
                        } => Some(domain),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if regexes.len() == children.len() {
                    Self::AnyRegexes(RegexSet::new(regexes)?)
                } else if domains.len() == children.len() {
                    Self::AnyDomains(RegexSet::new(domains)?)
                } else {
                    let mut matchers = vec![];
                    for child in children {
                        matchers.push(Self::from(child.clone())?);
                    }
                    Self::Any(matchers)
                }
            }
            PatternNode::All { children } => {
                let regexes = children
                    .iter()
                    .flat_map(|child| match child.as_ref() {
                        PatternNode::Leaf {
                            leaf: LinkPatternLeaf::Regex(regex),
                        } => Some(regex),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let domains = children
                    .iter()
                    .flat_map(|child| match child.as_ref() {
                        PatternNode::Leaf {
                            leaf: LinkPatternLeaf::Domain(domain),
                        } => Some(domain),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if regexes.len() == children.len() {
                    Self::AllRegexes(RegexSet::new(regexes)?)
                } else if domains.len() == children.len() {
                    Self::AllDomains(RegexSet::new(domains)?)
                } else {
                    let mut matchers = vec![];
                    for child in children {
                        matchers.push(Self::from(child.clone())?);
                    }
                    Self::All(matchers)
                }
            }
            PatternNode::Not { child } => Self::Not(Box::new(Self::from(child.clone())?)),
        })
    }
}

impl Matcher<&Url> for LinkMatcherInner {
    fn is_match(&self, url: &Url) -> bool {
        match self {
            Self::AnyRegexes(regexes) => regexes.is_match(url.as_str()),
            Self::AllRegexes(regexes) => {
                regexes.len() == regexes.matches(url.as_str()).into_iter().count()
            }
            Self::AnyDomains(domains) => {
                if let Some(domain) = url.domain() {
                    domains.is_match(domain)
                } else {
                    false
                }
            }
            Self::AllDomains(domains) => {
                if let Some(domain) = url.domain() {
                    domains.len() == domains.matches(domain).into_iter().count()
                } else {
                    false
                }
            }
            Self::Any(children) => children.iter().any(|child| child.is_match(url)),
            Self::All(children) => children.iter().all(|child| child.is_match(url)),
            Self::Not(child) => !child.is_match(url),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinkMatcher(Arc<LinkMatcherInner>);

impl Matcher<&Url> for LinkMatcher {
    fn is_match(&self, url: &Url) -> bool {
        self.0.is_match(url)
    }
}

impl CompileMatcher<LinkMatcher> for LinkPattern {
    fn compile(&self) -> Result<LinkMatcher> {
        Ok(LinkMatcher(Arc::new(LinkMatcherInner::from(
            compiler::optimize(Rc::<PatternNode<LinkPatternLeaf>>::from(self))?,
        )?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn test_multiple_types_of_matcher() {
        let pattern = LinkPattern::Any {
            any: vec![
                LinkPattern::Word {
                    word: "casino".to_string(),
                },
                LinkPattern::Domain {
                    domain: "spam.test".to_string(),
                },
            ],
        };

        let matcher = pattern.compile().expect("Couldn't compile");
        assert!(matcher.is_match(&Url::parse("https://link.to/casino").unwrap()));
        assert!(matcher.is_match(&Url::parse("https://spam.test/gamble").unwrap()));
        assert!(!matcher.is_match(&Url::parse("https://example.test/legit").unwrap()));
    }
}
