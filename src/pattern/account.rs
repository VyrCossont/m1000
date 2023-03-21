use crate::config::{AccountPattern, TextPattern, UserPattern};
use crate::pattern::compiler::{optimize, PatternNode};
use crate::pattern::text::{TextMatcher, TextMatcherInput};
use crate::pattern::user::{UserMatcher, UserMatcherInput};
use crate::pattern::{CompileMatcher, Matcher};
use anyhow::Result;
use mastodon_async::entities::account::Account;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone)]
enum AccountPatternLeaf {
    User(UserPattern),
    Text(TextPattern),
}

impl From<&AccountPattern> for Rc<PatternNode<AccountPatternLeaf>> {
    fn from(p: &AccountPattern) -> Rc<PatternNode<AccountPatternLeaf>> {
        Rc::new(match p {
            AccountPattern::User { user } => PatternNode::Leaf {
                leaf: AccountPatternLeaf::User(user.clone()),
            },
            AccountPattern::Text { text } => PatternNode::Leaf {
                leaf: AccountPatternLeaf::Text(text.clone()),
            },
            AccountPattern::Any { any } => PatternNode::Any {
                children: any.into_iter().map(|x| Self::from(x)).collect(),
            },
            AccountPattern::All { all } => PatternNode::All {
                children: all.into_iter().map(|x| Self::from(x)).collect(),
            },
            AccountPattern::Not { not } => PatternNode::Not {
                child: Self::from(not.as_ref()),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct AccountMatcher(Arc<AccountMatcherInner>);

#[derive(Debug, Clone)]
enum AccountMatcherInner {
    User(UserMatcher),
    Text(TextMatcher),
    Any(Vec<Self>),
    All(Vec<Self>),
    Not(Box<Self>),
}

impl AccountMatcherInner {
    pub fn from(node: Rc<PatternNode<AccountPatternLeaf>>) -> Result<Self> {
        Ok(match node.as_ref() {
            PatternNode::Leaf {
                leaf: AccountPatternLeaf::User(pattern),
            } => Self::User(pattern.compile()?),
            PatternNode::Leaf {
                leaf: AccountPatternLeaf::Text(pattern),
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
pub struct AccountMatcherInput {
    user: UserMatcherInput,
    text: TextMatcherInput,
}

impl From<&Account> for AccountMatcherInput {
    fn from(account: &Account) -> Self {
        Self {
            user: UserMatcherInput::from(account),
            text: TextMatcherInput::from(account),
        }
    }
}

impl Matcher<&AccountMatcherInput> for AccountMatcherInner {
    fn is_match(&self, input: &AccountMatcherInput) -> bool {
        match self {
            Self::User(matcher) => matcher.is_match(&input.user),
            Self::Text(matcher) => matcher.is_match(&input.text),
            Self::Any(children) => children.iter().any(|child| child.is_match(input)),
            Self::All(children) => children.iter().all(|child| child.is_match(input)),
            Self::Not(child) => !child.is_match(input),
        }
    }
}

impl Matcher<&AccountMatcherInput> for AccountMatcher {
    fn is_match(&self, input: &AccountMatcherInput) -> bool {
        self.0.is_match(input)
    }
}

impl CompileMatcher<AccountMatcher> for AccountPattern {
    fn compile(&self) -> Result<AccountMatcher> {
        Ok(AccountMatcher(Arc::new(AccountMatcherInner::from(
            optimize(Rc::<PatternNode<AccountPatternLeaf>>::from(self))?,
        )?)))
    }
}
