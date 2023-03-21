mod account;
mod compiler;
mod instance;
mod link;
mod post;
mod regex;
mod rule;
mod string;
mod text;
mod user;

use anyhow::Result;

pub trait Matcher<T> {
    fn is_match(&self, t: T) -> bool;
}

pub trait CompileMatcher<M> {
    fn compile(&self) -> Result<M>;
}

pub use rule::{RuleMatcher, RuleMatcherInput};
