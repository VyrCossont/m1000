use crate::config::{InstancePattern, StringPattern, UserPattern};
use crate::pattern::compiler::{optimize, PatternNode};
use crate::pattern::instance::InstanceMatcher;
use crate::pattern::string::StringMatcher;
use crate::pattern::{CompileMatcher, Matcher};
use anyhow::Result;
use mastodon_async::entities::{account::Account, mention::Mention};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone)]
enum UserPatternLeaf {
    Username(StringPattern),
    Instance(InstancePattern),
    Local(bool),
}

impl From<&UserPattern> for Rc<PatternNode<UserPatternLeaf>> {
    fn from(p: &UserPattern) -> Rc<PatternNode<UserPatternLeaf>> {
        Rc::new(match p {
            UserPattern::Username { username } => PatternNode::Leaf {
                leaf: UserPatternLeaf::Username(username.clone()),
            },
            UserPattern::Instance { instance } => PatternNode::Leaf {
                leaf: UserPatternLeaf::Instance(instance.clone()),
            },
            UserPattern::Local { local } => PatternNode::Leaf {
                leaf: UserPatternLeaf::Local(*local),
            },
            UserPattern::Any { any } => PatternNode::Any {
                children: any.into_iter().map(|x| Self::from(x)).collect(),
            },
            UserPattern::All { all } => PatternNode::All {
                children: all.into_iter().map(|x| Self::from(x)).collect(),
            },
            UserPattern::Not { not } => PatternNode::Not {
                child: Self::from(not.as_ref()),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct UserMatcher(Arc<UserMatcherInner>);

#[derive(Debug, Clone)]
enum UserMatcherInner {
    Username(StringMatcher),
    Instance(InstanceMatcher),
    Local(bool),
    Any(Vec<Self>),
    All(Vec<Self>),
    Not(Box<Self>),
}

impl UserMatcherInner {
    pub fn from(node: Rc<PatternNode<UserPatternLeaf>>) -> Result<Self> {
        Ok(match node.as_ref() {
            PatternNode::Leaf {
                leaf: UserPatternLeaf::Username(string_pattern),
            } => Self::Username(string_pattern.compile()?),
            PatternNode::Leaf {
                leaf: UserPatternLeaf::Instance(instance_pattern),
            } => Self::Instance(instance_pattern.compile()?),
            PatternNode::Leaf {
                leaf: UserPatternLeaf::Local(local),
            } => Self::Local(*local),
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserMatcherInput {
    username: String,
    domain: Option<String>,
}

impl From<&Mention> for UserMatcherInput {
    fn from(mention: &Mention) -> Self {
        Self {
            username: mention.username.clone(),
            domain: match mention.acct.splitn(2, '@').collect::<Vec<_>>().as_slice() {
                [_, domain] => Some(domain.to_string()),
                _ => None,
            },
        }
    }
}

impl From<&Account> for UserMatcherInput {
    fn from(account: &Account) -> Self {
        Self {
            username: account.username.clone(),
            domain: match account.acct.splitn(2, '@').collect::<Vec<_>>().as_slice() {
                [_, domain] => Some(domain.to_string()),
                _ => None,
            },
        }
    }
}

impl Matcher<&UserMatcherInput> for UserMatcherInner {
    fn is_match(&self, input: &UserMatcherInput) -> bool {
        match self {
            Self::Username(matcher) => matcher.is_match(&input.username),
            Self::Instance(matcher) => match input.domain.as_ref() {
                Some(domain) => matcher.is_match(domain),
                _ => false,
            },
            Self::Local(local) => *local == input.domain.is_none(),
            Self::Any(children) => children.iter().any(|child| child.is_match(input)),
            Self::All(children) => children.iter().all(|child| child.is_match(input)),
            Self::Not(child) => !child.is_match(input),
        }
    }
}

impl Matcher<&UserMatcherInput> for UserMatcher {
    fn is_match(&self, input: &UserMatcherInput) -> bool {
        self.0.is_match(input)
    }
}

impl CompileMatcher<UserMatcher> for UserPattern {
    fn compile(&self) -> Result<UserMatcher> {
        Ok(UserMatcher(Arc::new(UserMatcherInner::from(optimize(
            Rc::<PatternNode<UserPatternLeaf>>::from(self),
        )?)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mastodon_async::entities::AccountId;

    #[test]
    fn test_username_match() {
        let pattern = UserPattern::Username {
            username: StringPattern::Word {
                word: "thegx".to_string(),
            },
        };

        let mention = Mention {
            url: "https://instance.test/@thegx".to_string(),
            username: "thegx".to_string(),
            acct: "thegx@instance.test".to_string(),
            id: AccountId::new("123"),
        };

        let input = UserMatcherInput::from(&mention);
        let matcher = pattern.compile().expect("Couldn't compile");
        assert!(matcher.is_match(&input));
    }
}
