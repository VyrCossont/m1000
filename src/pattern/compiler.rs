use anyhow::{bail, Result};
use std::rc::Rc;

/// Intermediate representation of an expression made up of leaf matchers and boolean operators.
/// Leaf matchers might be regex patterns for strings, or other things for Mastodon API structures.
#[derive(Debug, Clone)]
pub enum PatternNode<L: Clone> {
    Leaf { leaf: L },
    Any { children: Vec<Rc<PatternNode<L>>> },
    All { children: Vec<Rc<PatternNode<L>>> },
    Not { child: Rc<PatternNode<L>> },
}

trait Visitable: Clone {
    fn visit<F>(self, f: F) -> Option<Self>
    where
        F: Fn(Self) -> Option<Self>;

    fn count(&self) -> usize;
}

impl<L: Clone> Visitable for Rc<PatternNode<L>> {
    fn visit<F>(self, f: F) -> Option<Self>
    where
        F: Fn(Self) -> Option<Self>,
    {
        match self.as_ref() {
            PatternNode::Leaf { .. } => f(self),
            PatternNode::Any { children } => {
                let children = children
                    .iter()
                    .flat_map(|child| f(child.clone()))
                    .collect::<Vec<_>>();
                f(Rc::new(PatternNode::Any { children }))
            }
            PatternNode::All { children } => {
                let children = children
                    .iter()
                    .flat_map(|child| f(child.clone()))
                    .collect::<Vec<_>>();
                f(Rc::new(PatternNode::All { children }))
            }
            PatternNode::Not { child } => match f(child.clone()) {
                None => None,
                Some(child) => f(Rc::new(PatternNode::Not {
                    child: child.clone(),
                })),
            },
        }
    }

    fn count(&self) -> usize {
        match self.as_ref() {
            PatternNode::Leaf { .. } => 1,
            PatternNode::Any { children } | PatternNode::All { children } => {
                1 + children.iter().map(|child| child.count()).sum::<usize>()
            }
            PatternNode::Not { child } => 1 + child.count(),
        }
    }
}

pub fn optimize<L: Clone>(root: Rc<PatternNode<L>>) -> Result<Rc<PatternNode<L>>> {
    let rules: Vec<fn(Rc<PatternNode<L>>) -> Option<Rc<PatternNode<L>>>> = vec![
        drop_empty,
        collapse_double_negative,
        pull_up_single_child,
        pull_up_same_type,
        de_morgan,
    ];

    let mut current = root.clone();
    let mut progress = true;
    while progress {
        progress = false;
        let num_nodes = current.count();
        for rule in rules.iter() {
            if let Some(applied) = current.clone().visit(rule) {
                if applied.count() < num_nodes {
                    current = applied.clone();
                    progress = true;
                    break;
                }
            } else {
                bail!("Reduced to nothingness");
            }
        }
    }

    Ok(current)
}

fn drop_empty<L: Clone>(node: Rc<PatternNode<L>>) -> Option<Rc<PatternNode<L>>> {
    match node.as_ref() {
        PatternNode::Any { children } | PatternNode::All { children } => {
            if children.is_empty() {
                None
            } else {
                Some(node.clone())
            }
        }
        _ => Some(node),
    }
}

fn collapse_double_negative<L: Clone>(node: Rc<PatternNode<L>>) -> Option<Rc<PatternNode<L>>> {
    match node.as_ref() {
        PatternNode::Not { child } => match child.as_ref() {
            PatternNode::Not { child: grandchild } => Some(grandchild.clone()),
            _ => Some(node),
        },
        _ => Some(node),
    }
}

fn pull_up_single_child<L: Clone>(node: Rc<PatternNode<L>>) -> Option<Rc<PatternNode<L>>> {
    match node.as_ref() {
        PatternNode::Any { children } | PatternNode::All { children } => {
            match children.as_slice() {
                [single] => Some(single.clone()),
                _ => Some(node.clone()),
            }
        }
        _ => Some(node),
    }
}

fn pull_up_same_type<L: Clone>(node: Rc<PatternNode<L>>) -> Option<Rc<PatternNode<L>>> {
    match node.as_ref() {
        PatternNode::Any { children } => Some(Rc::new(PatternNode::Any {
            children: children
                .into_iter()
                .flat_map(|child| match child.as_ref() {
                    PatternNode::Any {
                        children: grandchildren,
                    } => grandchildren.iter().collect(),
                    _ => vec![child],
                })
                .map(|child| child.clone())
                .collect(),
        })),
        PatternNode::All { children } => Some(Rc::new(PatternNode::All {
            children: children
                .into_iter()
                .flat_map(|child| match child.as_ref() {
                    PatternNode::All {
                        children: grandchildren,
                    } => grandchildren.iter().collect(),
                    _ => vec![child],
                })
                .map(|child| child.clone())
                .collect(),
        })),
        _ => Some(node.clone()),
    }
}

fn de_morgan<L: Clone>(node: Rc<PatternNode<L>>) -> Option<Rc<PatternNode<L>>> {
    match node.as_ref() {
        PatternNode::Any { children } => {
            let mut grandchildren = vec![];
            for child in children {
                match child.as_ref() {
                    PatternNode::Not { child: grandchild } => {
                        grandchildren.push(grandchild.clone());
                    }
                    _ => return Some(node.clone()),
                }
            }
            Some(Rc::new(PatternNode::Not {
                child: Rc::new(PatternNode::All {
                    children: grandchildren,
                }),
            }))
        }
        PatternNode::All { children } => {
            let mut grandchildren = vec![];
            for child in children {
                match child.as_ref() {
                    PatternNode::Not { child: grandchild } => {
                        grandchildren.push(grandchild.clone());
                    }
                    _ => return Some(node.clone()),
                }
            }
            Some(Rc::new(PatternNode::Not {
                child: Rc::new(PatternNode::Any {
                    children: grandchildren,
                }),
            }))
        }
        _ => Some(node.clone()),
    }
}
