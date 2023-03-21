use crate::config::{AccountPattern, PostPattern, RulePattern};
use crate::pattern::account::{AccountMatcher, AccountMatcherInput};
use crate::pattern::compiler::{optimize, PatternNode};
use crate::pattern::post::{PostMatcher, PostMatcherInput};
use crate::pattern::{CompileMatcher, Matcher};
use anyhow::Result;
use mastodon_async::entities::status::Status;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone)]
enum RulePatternLeaf {
    Account(AccountPattern),
    Post(PostPattern),
    Rspamd(String),
}

impl From<&RulePattern> for Rc<PatternNode<RulePatternLeaf>> {
    fn from(p: &RulePattern) -> Rc<PatternNode<RulePatternLeaf>> {
        Rc::new(match p {
            RulePattern::Account { account } => PatternNode::Leaf {
                leaf: RulePatternLeaf::Account(account.clone()),
            },
            RulePattern::Post { post } => PatternNode::Leaf {
                leaf: RulePatternLeaf::Post(post.clone()),
            },
            RulePattern::Rspamd { action } => PatternNode::Leaf {
                leaf: RulePatternLeaf::Rspamd(action.clone()),
            },
            RulePattern::Any { any } => PatternNode::Any {
                children: any.into_iter().map(|x| Self::from(x)).collect(),
            },
            RulePattern::All { all } => PatternNode::All {
                children: all.into_iter().map(|x| Self::from(x)).collect(),
            },
            RulePattern::Not { not } => PatternNode::Not {
                child: Self::from(not.as_ref()),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct RuleMatcher(Arc<RuleMatcherInner>);

#[derive(Debug, Clone)]
enum RuleMatcherInner {
    Account(AccountMatcher),
    Post(PostMatcher),
    Rspamd(String),
    Any(Vec<Self>),
    All(Vec<Self>),
    Not(Box<Self>),
}

impl RuleMatcherInner {
    pub fn from(node: Rc<PatternNode<RulePatternLeaf>>) -> Result<Self> {
        Ok(match node.as_ref() {
            PatternNode::Leaf {
                leaf: RulePatternLeaf::Account(pattern),
            } => Self::Account(pattern.compile()?),
            PatternNode::Leaf {
                leaf: RulePatternLeaf::Post(pattern),
            } => Self::Post(pattern.compile()?),
            PatternNode::Leaf {
                leaf: RulePatternLeaf::Rspamd(action),
            } => Self::Rspamd(action.clone()),
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
pub struct RuleMatcherInput {
    account: AccountMatcherInput,
    post: PostMatcherInput,
    rspamd: Option<String>,
}

impl From<&Status> for RuleMatcherInput {
    fn from(status: &Status) -> Self {
        Self {
            account: AccountMatcherInput::from(&status.account),
            post: PostMatcherInput::from(status),
            // If rspamd is enabled, this can be added later.
            rspamd: None,
        }
    }
}

impl RuleMatcherInput {
    pub fn rspamd(&mut self, action: String) -> &mut Self {
        self.rspamd = Some(action);
        self
    }
}

impl Matcher<&RuleMatcherInput> for RuleMatcherInner {
    fn is_match(&self, input: &RuleMatcherInput) -> bool {
        match self {
            Self::Account(matcher) => matcher.is_match(&input.account),
            Self::Post(matcher) => matcher.is_match(&input.post),
            Self::Rspamd(action) => input
                .rspamd
                .as_ref()
                .map(|input_action| action == input_action)
                .unwrap_or(false),
            Self::Any(children) => children.iter().any(|child| child.is_match(input)),
            Self::All(children) => children.iter().all(|child| child.is_match(input)),
            Self::Not(child) => !child.is_match(input),
        }
    }
}

impl Matcher<&RuleMatcherInput> for RuleMatcher {
    fn is_match(&self, input: &RuleMatcherInput) -> bool {
        self.0.is_match(input)
    }
}

impl CompileMatcher<RuleMatcher> for RulePattern {
    fn compile(&self) -> Result<RuleMatcher> {
        Ok(RuleMatcher(Arc::new(RuleMatcherInner::from(optimize(
            Rc::<PatternNode<RulePatternLeaf>>::from(self),
        )?)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LinkPattern, TextPattern};
    use mastodon_async::entities::{account::Account, AccountId, StatusId};
    use time::OffsetDateTime;
    use url::Url;

    #[test]
    fn test_example_rule() {
        let pattern = RulePattern::Post {
            post: PostPattern::Text {
                text: TextPattern::Link {
                    link: LinkPattern::Domain {
                        domain: "news.ycombinator.com".to_string(),
                    },
                },
            },
        };

        let matcher = pattern.compile().expect("Couldn't compile");

        let input = RuleMatcherInput::from(&Status {
            id: StatusId::new(""),
            uri: Url::parse("https://example.test").unwrap(),
            url: None,
            account: Account {
                acct: "".to_string(),
                avatar: Url::parse("https://example.test").unwrap(),
                avatar_static: Url::parse("https://example.test").unwrap(),
                bot: false,
                created_at: OffsetDateTime::UNIX_EPOCH,
                discoverable: None,
                display_name: "".to_string(),
                emojis: vec![],
                fields: vec![],
                followers_count: 0,
                following_count: 0,
                group: false,
                header: Url::parse("https://example.test").unwrap(),
                header_static: Url::parse("https://example.test").unwrap(),
                id: AccountId::new(""),
                last_status_at: None,
                limited: false,
                locked: false,
                moved: None,
                no_index: None,
                note: "".to_string(),
                source: None,
                statuses_count: 0,
                suspended: false,
                url: Url::parse("https://example.test").unwrap(),
                username: "".to_string(),
            },
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            reblog: None,
            content: r#"<p>Guidelines for Brutalist Web Design<br />L: <a href="https://brutalist-web.design/" target="_blank" rel="nofollow noopener noreferrer"><span class="invisible">https://</span><span class="">brutalist-web.design/</span><span class="invisible"></span></a><br />C: <a href="https://news.ycombinator.com/item?id=35783189" target="_blank" rel="nofollow noopener noreferrer"><span class="invisible">https://</span><span class="ellipsis">news.ycombinator.com/item?id=3</span><span class="invisible">5783189</span></a></p>"#.to_string(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            edited_at: None,
            emojis: vec![],
            replies_count: 0,
            reblogs_count: 0,
            favourites_count: 0,
            reblogged: None,
            favourited: None,
            muted: None,
            bookmarked: None,
            pinned: None,
            sensitive: false,
            spoiler_text: "".to_string(),
            visibility: Default::default(),
            media_attachments: vec![],
            mentions: vec![],
            tags: vec![],
            application: None,
            language: None,
            poll: None,
            card: None,
            text: None,
            filtered: vec![],
        });

        assert!(matcher.is_match(&input));
    }
}
