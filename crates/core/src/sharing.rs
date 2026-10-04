//! Разрешения автора сохраняются отдельно от истории передач (историю можно очистить).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::engine::{Event, Inner};
use crate::model::{FileAccess, HistoryShare, OfferFile};
use crate::store::State;
use crate::util::top_item;
use anyhow::{Result, bail};

/// Миграция старого состояния: полученный файл остаётся файлом его отправителя.
pub(crate) fn migrate(s: &mut State, me: &str) {
    let peers: Vec<String> = s.group.active().map(|m| m.id.clone()).collect();
    for path in s.index.keys() {
        if s.file_access.contains_key(path) {
            continue;
        }
        let received = s
            .incoming
            .iter()
            .rev()
            .find(|i| i.saved.contains(path) || i.offer.files.iter().any(|f| f.path == *path));
        // Если история уже очищена, автора старого файла надёжно восстановить нельзя.
        // Не приписывать такой файл себе: это мог быть чужой полученный документ.
        let owner = received.map(|i| i.offer.from.clone()).unwrap_or_default();
        // Старые полученные файлы: только отправитель и этот компьютер. Не угадывать третьих.
        let audience = if received.is_some() {
            vec![me.to_string(), owner.clone()]
        } else {
            peers.clone()
        };
        s.file_access
            .insert(path.clone(), FileAccess { owner, audience });
    }
}

/// Зафиксировать старые пути до изменения состава семьи, даже если хеширование ещё идёт.
pub(crate) fn seed_existing(inner: &Arc<Inner>) {
    let root = inner.root();
    let listing = crate::scan::walk(&root, &root);
    let mut s = inner.st();
    migrate(&mut s, &inner.me);
    let audience: Vec<String> = s.group.active().map(|m| m.id.clone()).collect();
    for (path, _, _) in listing {
        s.file_access.entry(path).or_insert_with(|| FileAccess {
            owner: inner.me.clone(),
            audience: audience.clone(),
        });
    }
}

pub(crate) fn queue(inner: &Arc<Inner>, peer: &str, added_by: &str) {
    let root = inner.root();
    let event = {
        let mut s = inner.st();
        migrate(&mut s, &inner.me);
        if !s.group.is_member(peer)
            || peer == inner.me
            || s.history_shares.iter().any(|r| r.peer == peer)
        {
            return;
        }
        let paths: Vec<String> =
            s.file_access
                .keys()
                .filter(|p| {
                    s.file_access.get(*p).is_some_and(|a| {
                        a.owner == inner.me && !a.audience.iter().any(|id| id == peer)
                    }) && crate::scan::native(&root, p).is_file()
                })
                .cloned()
                .collect();
        let files = paths.len();
        if files > 0 {
            s.history_shares.push(HistoryShare {
                peer: peer.into(),
                added_by: added_by.into(),
                paths,
            });
        }
        Event::Joined {
            id: peer.into(),
            name: s.group.name_of(peer),
            added_by: s.group.name_of(added_by),
            files,
        }
    };
    inner.save_now();
    inner.emit(event);
    inner.changed();
}

/// Согласие относится только к собственным файлам из сохранённого запроса.
pub(crate) fn answer(inner: &Arc<Inner>, peer: &str, allow: bool) -> Result<()> {
    let groups = {
        let mut s = inner.st();
        if !s.group.is_member(peer) {
            bail!(crate::t!("share.gone"));
        }
        let Some(pos) = s.history_shares.iter().position(|r| r.peer == peer) else {
            return Ok(()); // повторное нажатие / ответ из другого окна
        };
        if allow
            && s.history_shares[pos].paths.iter().any(|p| {
                !s.index.contains_key(p) && crate::scan::native(&s.settings.folder, p).is_file()
            })
        {
            bail!(crate::t!("share.preparing"));
        }
        let request = s.history_shares.remove(pos);
        let mut groups: BTreeMap<String, Vec<OfferFile>> = BTreeMap::new();
        if allow {
            for path in request.paths {
                let entry = s.index.get(&path).cloned();
                let Some(access) = s.file_access.get_mut(&path) else {
                    continue;
                };
                if access.owner != inner.me {
                    continue;
                }
                if !access.audience.iter().any(|p| p == peer) {
                    access.audience.push(peer.into());
                }
                let Some(entry) = entry else { continue }; // ещё хешируется: сканер предложит после согласия
                groups
                    .entry(top_item(&path).into())
                    .or_default()
                    .push(OfferFile {
                        path,
                        size: entry.size,
                        mtime: entry.mtime,
                        hash: entry.hash,
                        prev_hash: None,
                        owner: access.owner.clone(),
                        audience: access.audience.clone(),
                    });
            }
        }
        groups
    };
    inner.save_now(); // согласие должно пережить перезапуск до создания предложений
    for (item, files) in groups {
        let folder = inner.root().join(&item).is_dir();
        crate::scan::create_offers(inner, &item, folder, files, Some(peer));
    }
    inner.changed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Group, IndexEntry, Member};

    #[test]
    fn migration_preserves_access_and_json_roundtrip() {
        let mut s = State::default();
        s.group = Group {
            members: vec![
                Member {
                    id: "me".into(),
                    name: "Me".into(),
                    added_by: String::new(),
                },
                Member {
                    id: "brother".into(),
                    name: "Brother".into(),
                    added_by: "me".into(),
                },
            ],
            ..Default::default()
        };
        s.index.insert(
            "own.txt".into(),
            IndexEntry {
                size: 1,
                mtime: 1,
                hash: "hash".into(),
            },
        );
        s.file_access.insert(
            "own.txt".into(),
            FileAccess {
                owner: "me".into(),
                audience: vec!["me".into(), "brother".into()],
            },
        );
        migrate(&mut s, "me");
        s.group.members.push(Member {
            id: "mom".into(),
            name: "Mom".into(),
            added_by: "brother".into(),
        });
        migrate(&mut s, "me");
        assert!(
            !s.file_access["own.txt"].audience.contains(&"mom".into()),
            "повторная миграция не расширяет разрешения после добавления участника"
        );
        s.history_shares.push(HistoryShare {
            peer: "mom".into(),
            added_by: "brother".into(),
            paths: vec!["own.txt".into()],
        });
        let restored: State = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(restored.history_shares[0].paths, vec!["own.txt"]);
        assert_eq!(restored.file_access["own.txt"].owner, "me");
        assert!(
            !restored.file_access["own.txt"]
                .audience
                .contains(&"mom".into())
        );
    }

    #[test]
    fn legacy_file_without_history_is_not_claimed_as_own() {
        let mut s = State::default();
        s.index.insert(
            "old.txt".into(),
            IndexEntry {
                size: 1,
                mtime: 1,
                hash: "hash".into(),
            },
        );
        migrate(&mut s, "me");
        assert!(
            s.file_access["old.txt"].owner.is_empty(),
            "очищенная история не даёт получателю права раздавать чужие файлы"
        );
    }
}
