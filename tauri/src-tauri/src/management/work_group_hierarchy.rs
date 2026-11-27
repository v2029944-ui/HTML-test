//! Stable parent references shared by settings transfer and analysis indexing.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub(super) const GROUP_PARENTS_FIELD: &str = "workGroupTreeParents";
const MAX_GROUP_DEPTH: usize = 32;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GroupParent {
    pub(super) child: String,
    pub(super) parent: String,
}

pub(super) fn validate_group_parents<'a>(
    ids: impl IntoIterator<Item = &'a str>,
    links: &[GroupParent],
) -> Result<(), String> {
    let mut known = HashSet::new();
    for id in ids {
        if id.trim().is_empty() || id.len() > 128 || !known.insert(id) {
            return Err("Идентификаторы групп дерева должны быть непустыми, уникальными и не длиннее 128 байт.".into());
        }
    }

    let mut parents = HashMap::new();
    for link in links {
        let child = link.child.as_str();
        let parent = link.parent.as_str();
        if !known.contains(child) || !known.contains(parent) {
            return Err("Ссылка на отсутствующую группу.".into());
        }
        if parents.insert(child, parent).is_some() {
            return Err("У группы может быть только один родитель.".into());
        }
    }

    for id in known {
        let mut seen = HashSet::new();
        let mut current = id;
        loop {
            if !seen.insert(current) {
                return Err("Нельзя создать цикл в дереве групп.".into());
            }
            if seen.len() > MAX_GROUP_DEPTH {
                return Err(format!("Максимальная глубина дерева: {MAX_GROUP_DEPTH}."));
            }
            let Some(&parent) = parents.get(current) else { break; };
            current = parent;
        }
    }
    Ok(())
}
