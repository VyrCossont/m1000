use crate::config::{PostPattern, TextPattern};
use crate::pattern::compiler::{optimize, PatternNode};
use crate::pattern::text::{TextMatcher, TextMatcherInput};
use crate::pattern::{CompileMatcher, Matcher};
use anyhow::Result;
use mastodon_async::entities::status::Status;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone)]
enum PostPatternLeaf {
    Text(TextPattern),
}

impl From<&PostPattern> for Rc<PatternNode<PostPatternLeaf>> {
    fn from(p: &PostPattern) -> Rc<PatternNode<PostPatternLeaf>> {
        Rc::new(match p {
            PostPattern::Text { text } => PatternNode::Leaf {
                leaf: PostPatternLeaf::Text(text.clone()),
            },
            PostPattern::Any { any } => PatternNode::Any {
                children: any.into_iter().map(|x| Self::from(x)).collect(),
            },
            PostPattern::All { all } => PatternNode::All {
                children: all.into_iter().map(|x| Self::from(x)).collect(),
            },
            PostPattern::Not { not } => PatternNode::Not {
                child: Self::from(not.as_ref()),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct PostMatcher(Arc<PostMatcherInner>);

#[derive(Debug, Clone)]
enum PostMatcherInner {
    Text(TextMatcher),
    Any(Vec<Self>),
    All(Vec<Self>),
    Not(Box<Self>),
}

impl PostMatcherInner {
    pub fn from(node: Rc<PatternNode<PostPatternLeaf>>) -> Result<Self> {
        Ok(match node.as_ref() {
            PatternNode::Leaf {
                leaf: PostPatternLeaf::Text(pattern),
            } => Self::Text(pattern.compile()?),
            PatternNode::Any { children } => {
                let mut matchers = vec![];
                for child in children {
                    matchers.push(Self::from(child.clone())?);
                }
                Self::Any(matchers)
            }
            PatternNode::All { children } => {
                let mut matchers = vec![];
                for child in children {
                    matchers.push(Self::from(child.clone())?);
                }
                Self::All(matchers)
            }
            PatternNode::Not { child } => Self::Not(Box::new(Self::from(child.clone())?)),
        })
    }
}

#[derive(Debug, Clone)]
pub struct PostMatcherInput {
    text: TextMatcherInput,
}

impl From<&Status> for PostMatcherInput {
    fn from(status: &Status) -> Self {
        Self {
            text: TextMatcherInput::from(status),
        }
    }
}

impl Matcher<&PostMatcherInput> for PostMatcherInner {
    fn is_match(&self, input: &PostMatcherInput) -> bool {
        match self {
            Self::Text(matcher) => matcher.is_match(&input.text),
            Self::Any(children) => children.iter().any(|child| child.is_match(input)),
            Self::All(children) => children.iter().all(|child| child.is_match(input)),
            Self::Not(child) => !child.is_match(input),
        }
    }
}

impl Matcher<&PostMatcherInput> for PostMatcher {
    fn is_match(&self, input: &PostMatcherInput) -> bool {
        self.0.is_match(input)
    }
}

impl CompileMatcher<PostMatcher> for PostPattern {
    fn compile(&self) -> Result<PostMatcher> {
        Ok(PostMatcher(Arc::new(PostMatcherInner::from(optimize(
            Rc::<PatternNode<PostPatternLeaf>>::from(self),
        )?)?)))
    }
}
