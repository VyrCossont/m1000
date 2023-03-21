use crate::config::{LinkPattern, StringPattern, TextPattern, UserPattern};
use crate::pattern::compiler::{optimize, PatternNode};
use crate::pattern::link::LinkMatcher;
use crate::pattern::string::StringMatcher;
use crate::pattern::user::{UserMatcher, UserMatcherInput};
use crate::pattern::{CompileMatcher, Matcher};
use anyhow::Result;
use lazy_static::lazy_static;
use mastodon_async::entities::{account::Account, status::Status};
use regex::RegexSet;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use twitter_text::extractor::{Extract, Extractor};
use url::Url;

#[derive(Debug, Clone)]
enum TextPatternLeaf {
    Regex(String),
    Link(LinkPattern),
    Mention(UserPattern),
    Hashtag(StringPattern),
}

impl From<&TextPattern> for Rc<PatternNode<TextPatternLeaf>> {
    fn from(p: &TextPattern) -> Rc<PatternNode<TextPatternLeaf>> {
        Rc::new(match p {
            TextPattern::Word { word } => PatternNode::Leaf {
                leaf: TextPatternLeaf::Regex(format!(
                    r"(?i:\b{word}\b)",
                    word = regex::escape(&word)
                )),
            },
            TextPattern::Regex { regex } => PatternNode::Leaf {
                leaf: TextPatternLeaf::Regex(regex.clone()),
            },
            TextPattern::Link { link } => PatternNode::Leaf {
                leaf: TextPatternLeaf::Link(link.clone()),
            },
            TextPattern::Mention { mention } => PatternNode::Leaf {
                leaf: TextPatternLeaf::Mention(mention.clone()),
            },
            TextPattern::Hashtag { hashtag } => PatternNode::Leaf {
                leaf: TextPatternLeaf::Hashtag(hashtag.clone()),
            },
            TextPattern::All { all } => PatternNode::All {
                children: all.into_iter().map(|x| Self::from(x)).collect(),
            },
            TextPattern::Any { any } => PatternNode::Any {
                children: any.into_iter().map(|x| Self::from(x)).collect(),
            },
            TextPattern::Not { not } => PatternNode::Not {
                child: Self::from(not.as_ref()),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct TextMatcher(Arc<TextMatcherInner>);

#[derive(Debug, Clone)]
enum TextMatcherInner {
    AllRegexes(RegexSet),
    AnyRegexes(RegexSet),
    Link(LinkMatcher),
    Mention(UserMatcher),
    Hashtag(StringMatcher),
    Any(Vec<Self>),
    All(Vec<Self>),
    Not(Box<Self>),
}

impl TextMatcherInner {
    pub fn from(node: Rc<PatternNode<TextPatternLeaf>>) -> Result<Self> {
        Ok(match node.as_ref() {
            PatternNode::Leaf {
                leaf: TextPatternLeaf::Regex(regex),
            } => Self::AnyRegexes(RegexSet::new(&[regex])?),
            PatternNode::Leaf {
                leaf: TextPatternLeaf::Link(pattern),
            } => Self::Link(pattern.compile()?),
            PatternNode::Leaf {
                leaf: TextPatternLeaf::Mention(pattern),
            } => Self::Mention(pattern.compile()?),
            PatternNode::Leaf {
                leaf: TextPatternLeaf::Hashtag(pattern),
            } => Self::Hashtag(pattern.compile()?),
            PatternNode::Any { children } => {
                let regexes = children
                    .iter()
                    .flat_map(|child| match child.as_ref() {
                        PatternNode::Leaf {
                            leaf: TextPatternLeaf::Regex(regex),
                        } => Some(regex),
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
            PatternNode::All { children } => {
                let regexes = children
                    .iter()
                    .flat_map(|child| match child.as_ref() {
                        PatternNode::Leaf {
                            leaf: TextPatternLeaf::Regex(regex),
                        } => Some(regex),
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
            PatternNode::Not { child } => Self::Not(Box::new(Self::from(child.clone())?)),
        })
    }
}

#[derive(Debug, Clone)]
pub struct TextMatcherInput {
    text: String,
    links: HashSet<Url>,
    mentions: HashSet<UserMatcherInput>,
    hashtags: HashSet<String>,
}

impl TextMatcherInput {
    fn extend_text(&mut self, s: &str) -> &mut Self {
        self.text.push(' ');
        self.text.push_str(s);
        self
    }

    fn merge(&mut self, other: TextMatcherInput) -> &mut Self {
        self.extend_text(&other.text);
        self.links.extend(other.links);
        self.mentions.extend(other.mentions);
        self.hashtags.extend(other.hashtags);
        self
    }
}

lazy_static! {
    static ref LINK_SELECTOR: Selector = Selector::parse("a[href]").unwrap();
}

impl From<&Html> for TextMatcherInput {
    fn from(html: &Html) -> Self {
        let text = html
            .root_element()
            .descendants()
            .filter_map(|node| node.value().as_text())
            .map(|text| text.text.to_string())
            .collect::<Vec<_>>()
            .join("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        let links = html
            .select(&LINK_SELECTOR)
            .filter_map(|a| Url::parse(a.value().attr("href").unwrap()).ok())
            .collect();

        Self {
            text,
            links,
            mentions: Default::default(),
            hashtags: Default::default(),
        }
    }
}

impl From<&Account> for TextMatcherInput {
    fn from(account: &Account) -> Self {
        let bio = Html::parse_fragment(&account.note);
        let mut input = Self::from(&bio);

        // The Mastodon API doesn't surface hashtags in account bios like it does for posts.
        // TODO: It doesn't surface mentions either.
        input.hashtags.extend(
            Extractor::new()
                .extract_hashtags(&input.text)
                .iter()
                .map(|tag| tag.value.to_string()),
        );

        input.extend_text(&account.display_name);

        for field in account.fields.iter() {
            input.extend_text(&field.name);

            let value = Html::parse_fragment(&field.value);
            input.merge(TextMatcherInput::from(&value));
        }

        input
    }
}

impl From<&Status> for TextMatcherInput {
    fn from(status: &Status) -> Self {
        let content = Html::parse_fragment(&status.content);
        let mut input = Self::from(&content);

        input.extend_text(&status.spoiler_text);

        for attachment in status.media_attachments.iter() {
            if let Some(description) = attachment.description.as_ref() {
                input.extend_text(description);
            }
        }

        if let Some(poll) = status.poll.as_ref() {
            for option in poll.options.iter() {
                input.extend_text(&option.title);
            }
        }

        input
            .mentions
            .extend(status.mentions.iter().map(UserMatcherInput::from));

        input
            .hashtags
            .extend(status.tags.iter().map(|tag| tag.name.to_string()));

        input
    }
}

impl Matcher<&TextMatcherInput> for TextMatcherInner {
    fn is_match(&self, input: &TextMatcherInput) -> bool {
        match self {
            Self::AllRegexes(regexes) => {
                regexes.len() == regexes.matches(&input.text).into_iter().count()
            }
            Self::AnyRegexes(regexes) => regexes.is_match(&input.text),
            Self::Link(matcher) => input.links.iter().any(|url| matcher.is_match(url)),
            Self::Mention(matcher) => input
                .mentions
                .iter()
                .any(|mention| matcher.is_match(mention)),
            Self::Hashtag(matcher) => input
                .hashtags
                .iter()
                .any(|hashtag| matcher.is_match(hashtag)),
            Self::Any(children) => children.iter().any(|child| child.is_match(input)),
            Self::All(children) => children.iter().all(|child| child.is_match(input)),
            Self::Not(child) => !child.is_match(input),
        }
    }
}

impl Matcher<&TextMatcherInput> for TextMatcher {
    fn is_match(&self, input: &TextMatcherInput) -> bool {
        self.0.is_match(input)
    }
}

impl CompileMatcher<TextMatcher> for TextPattern {
    fn compile(&self) -> Result<TextMatcher> {
        Ok(TextMatcher(Arc::new(TextMatcherInner::from(optimize(
            Rc::<PatternNode<TextPatternLeaf>>::from(self),
        )?)?)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use mastodon_async::entities::AccountId;
    use time::OffsetDateTime;

    #[test]
    fn test_extract_account_hashtags() {
        let account = Account {
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
            note: r#"<p>Been working on webhooks for the moderation API...</p><p><a href="https://github.com/mastodon/mastodon/pull/18510" target="_blank" rel="nofollow noopener noreferrer"><span class="invisible">https://</span><span class="ellipsis">github.com/mastodon/mastodon/p</span><span class="invisible">ull/18510</span></a> <a href="https://mastodon.social/tags/mastodev" class="mention hashtag" rel="tag">#<span>mastodev</span></a></p>"#.to_string(),
            source: None,
            statuses_count: 0,
            suspended: false,
            url: Url::parse("https://example.test").unwrap(),
            username: "".to_string(),
        };

        let input = TextMatcherInput::from(&account);
        assert_eq!(input.hashtags, HashSet::from(["mastodev".to_string()]));
    }
}
