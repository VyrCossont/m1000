use crate::config::StringPattern;
use crate::pattern::compiler::{self, PatternNode};
use crate::pattern::regex::RegexPatternMatcher;
use crate::pattern::{CompileMatcher, Matcher};
use anyhow::Result;
use std::rc::Rc;
use std::sync::Arc;

impl From<&StringPattern> for Rc<PatternNode<String>> {
    fn from(p: &StringPattern) -> Rc<PatternNode<String>> {
        Rc::new(match p {
            StringPattern::Word { word } => PatternNode::Leaf {
                leaf: format!(r"(?i:\b{word}\b)", word = regex::escape(&word)),
            },
            StringPattern::Regex { regex } => PatternNode::Leaf {
                leaf: regex.clone(),
            },
            StringPattern::Any { any } => PatternNode::Any {
                children: any.into_iter().map(|x| Self::from(x)).collect(),
            },
            StringPattern::All { all } => PatternNode::All {
                children: all.into_iter().map(|x| Self::from(x)).collect(),
            },
            StringPattern::Not { not } => PatternNode::Not {
                child: Self::from(not.as_ref()),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct StringMatcher(Arc<RegexPatternMatcher>);

impl Matcher<&str> for StringMatcher {
    fn is_match(&self, s: &str) -> bool {
        self.0.is_match(s)
    }
}

impl CompileMatcher<StringMatcher> for StringPattern {
    fn compile(&self) -> Result<StringMatcher> {
        Ok(StringMatcher(Arc::new(RegexPatternMatcher::from(
            compiler::optimize(Rc::<PatternNode<String>>::from(self))?,
        )?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_several_compiler_rules() {
        let pattern = StringPattern::Not {
            not: Box::new(StringPattern::All {
                all: vec![
                    StringPattern::Not {
                        not: Box::new(StringPattern::Word {
                            word: "foo".to_string(),
                        }),
                    },
                    StringPattern::Not {
                        not: Box::new(StringPattern::Word {
                            word: "bar".to_string(),
                        }),
                    },
                ],
            }),
        };

        let matcher = pattern.compile().expect("Couldn't compile");
        match &matcher.0.as_ref() {
            RegexPatternMatcher::AnyRegexes(regexes) => {
                assert_eq!(2, regexes.len())
            }
            _ => assert!(false, "Unexpected variant for compiled pattern matcher"),
        }
        assert!(matcher.is_match("foo"));
        assert!(matcher.is_match("BAR"));
    }
}
