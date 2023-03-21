use crate::config::InstancePattern;
use crate::pattern::compiler::{self, PatternNode};
use crate::pattern::regex::RegexPatternMatcher;
use crate::pattern::{CompileMatcher, Matcher};
use anyhow::Result;
use std::rc::Rc;
use std::sync::Arc;

impl From<&InstancePattern> for Rc<PatternNode<String>> {
    fn from(p: &InstancePattern) -> Rc<PatternNode<String>> {
        Rc::new(match p {
            InstancePattern::Word { word } => PatternNode::Leaf {
                leaf: format!(r"(?i:\b{word}\b)", word = regex::escape(&word)),
            },
            InstancePattern::Regex { regex } => PatternNode::Leaf {
                leaf: regex.clone(),
            },
            InstancePattern::Domain { domain } => PatternNode::Leaf {
                leaf: format!(r"(?i:\b{domain}$)", domain = regex::escape(&domain)),
            },
            InstancePattern::All { all } => PatternNode::All {
                children: all.into_iter().map(|x| Self::from(x)).collect(),
            },
            InstancePattern::Any { any } => PatternNode::Any {
                children: any.into_iter().map(|x| Self::from(x)).collect(),
            },
            InstancePattern::Not { not } => PatternNode::Not {
                child: Self::from(not.as_ref()),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct InstanceMatcher(Arc<RegexPatternMatcher>);

impl Matcher<&str> for InstanceMatcher {
    fn is_match(&self, s: &str) -> bool {
        self.0.is_match(s)
    }
}

impl CompileMatcher<InstanceMatcher> for InstancePattern {
    fn compile(&self) -> Result<InstanceMatcher> {
        Ok(InstanceMatcher(Arc::new(RegexPatternMatcher::from(
            compiler::optimize(Rc::<PatternNode<String>>::from(self))?,
        )?)))
    }
}
