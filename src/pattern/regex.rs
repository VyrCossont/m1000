use crate::pattern::compiler::PatternNode;
use crate::pattern::Matcher;
use anyhow::Result;
use regex::RegexSet;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum RegexPatternMatcher {
    AnyRegexes(RegexSet),
    AllRegexes(RegexSet),
    Any(Vec<Self>),
    All(Vec<Self>),
    Not(Box<Self>),
}

impl RegexPatternMatcher {
    pub fn from(node: Rc<PatternNode<String>>) -> Result<Self> {
        Ok(match node.as_ref() {
            PatternNode::Leaf { leaf: regex } => Self::AnyRegexes(RegexSet::new(&[regex])?),
            PatternNode::All { children } => {
                let regexes = children
                    .iter()
                    .flat_map(|child| match child.as_ref() {
                        PatternNode::Leaf { leaf: regex } => Some(regex),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if regexes.len() == children.len() {
                    Self::AllRegexes(RegexSet::new(regexes)?)
                } else {
                    let mut matchers = vec![];
                    for child in children {
                        matchers.push(Self::from(child.clone())?);
                    }
                    Self::All(matchers)
                }
            }
            PatternNode::Any { children } => {
                let regexes = children
                    .iter()
                    .flat_map(|child| match child.as_ref() {
                        PatternNode::Leaf { leaf: regex } => Some(regex),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if regexes.len() == children.len() {
                    Self::AnyRegexes(RegexSet::new(regexes)?)
                } else {
                    let mut matchers = vec![];
                    for child in children {
                        matchers.push(Self::from(child.clone())?);
                    }
                    Self::Any(matchers)
                }
            }
            PatternNode::Not { child } => Self::Not(Box::new(Self::from(child.clone())?)),
        })
    }
}

impl Matcher<&str> for RegexPatternMatcher {
    fn is_match(&self, s: &str) -> bool {
        match self {
            Self::AnyRegexes(regexes) => regexes.is_match(s),
            Self::AllRegexes(regexes) => regexes.len() == regexes.matches(s).into_iter().count(),
            Self::Any(children) => children.iter().any(|child| child.is_match(s)),
            Self::All(children) => children.iter().all(|child| child.is_match(s)),
            Self::Not(child) => !child.is_match(s),
        }
    }
}
