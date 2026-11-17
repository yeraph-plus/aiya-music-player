//! 频道节目配置与系统歌单引用。配置写入同一数据库的 settings，媒体路径只供引擎渲染。
use crate::state::{setting_get, setting_set};
use aiya_data::{DbError, DbResult};
use aiya_dto::station::*;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, HashSet};
use std::hash::{Hash, Hasher};

const LIBRARY: &str = "__library__";

fn end_minutes(value: &str) -> Option<u32> {
    if value == "24:00" {
        Some(1440)
    } else {
        crate::station::hm_minutes(value)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    revision: u64,
    items: Vec<StationQueueItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub path: String,
    pub title: String,
}

pub struct Prepared {
    pub program: StationProgram,
    pub entries: Vec<Vec<Entry>>,
    pub fingerprint: String,
}

fn key(id: &str) -> String {
    format!("station.program.{id}")
}
fn applied_key(id: &str) -> String {
    format!("station.applied.{id}")
}
fn error_key(id: &str) -> String {
    format!("station.error.{id}")
}
fn invalid(msg: impl Into<String>) -> DbError {
    DbError::Custom(msg.into())
}

pub fn clear(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute(
        "DELETE FROM settings WHERE key IN (?1,?2,?3)",
        params![key(id), applied_key(id), error_key(id)],
    )?;
    Ok(())
}

fn read(conn: &Connection, id: &str) -> DbResult<Saved> {
    setting_get(conn, &key(id))
        .map(serde_json::from_value)
        .transpose()
        .map(|v| v.unwrap_or_default())
        .map_err(|e| invalid(format!("Invalid station program: {e}")))
}

pub fn prepare(conn: &Connection, id: &str) -> DbResult<Prepared> {
    let saved = read(conn, id)?;
    let mut entries = Vec::new();
    let library_count: u32 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?;
    let library_available: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE availability='online'",
        [],
        |r| r.get(0),
    )?;
    let mut sources = vec![StationQueueSource {
        playlist_id: LIBRARY.into(),
        name: "All tracks".into(),
        track_count: library_count,
        available_count: library_available,
        exists: true,
    }];
    let mut issues = Vec::new();
    for item in &saved.items {
        let library = item.playlist_id == LIBRARY;
        let name = if library {
            Some("All tracks".into())
        } else {
            conn.query_row(
                "SELECT name FROM playlists WHERE id=?1",
                [&item.playlist_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        };
        let total: u32 = if library {
            library_count
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM playlist_items WHERE playlist_id=?1",
                [&item.playlist_id],
                |r| r.get(0),
            )?
        };
        let mut stmt = conn.prepare(if library {
            "SELECT id,path,COALESCE(title,'') FROM tracks WHERE availability='online' ORDER BY title_norm,id"
        } else { "SELECT t.id,t.path,COALESCE(t.title,'') FROM playlist_items p JOIN tracks t ON t.id=p.track_id WHERE p.playlist_id=?1 AND t.availability='online' ORDER BY p.position" })?;
        let list = stmt
            .query_map(
                rusqlite::params_from_iter((!library).then_some(&item.playlist_id)),
                |r| {
                    Ok(Entry {
                        id: r.get(0)?,
                        path: r.get(1)?,
                        title: r.get(2)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let source = StationQueueSource {
            playlist_id: item.playlist_id.clone(),
            name: name.clone().unwrap_or_else(|| item.playlist_id.clone()),
            exists: name.is_some(),
            track_count: total,
            available_count: list.len() as u32,
        };
        if !source.exists {
            issues.push(StationProgramIssue::MissingSource {
                playlist_id: item.playlist_id.clone(),
            });
        } else if list.is_empty() {
            issues.push(StationProgramIssue::EmptySource {
                playlist_id: item.playlist_id.clone(),
            });
        }
        if !library {
            sources.push(source);
        }
        entries.push(list);
    }
    if saved.items.is_empty() {
        issues.push(StationProgramIssue::EmptyQueue);
    }
    if !saved.items.is_empty() && !saved.items.iter().any(|i| !i.scheduled) {
        let coverage = coverage(&saved.items)?;
        if let Some(m) = coverage.iter().position(|v| !v) {
            issues.push(StationProgramIssue::UncoveredTime {
                weekday: (m / 1440) as u8,
                time: format!("{:02}:{:02}", m % 1440 / 60, m % 60),
            });
        }
    }
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(&(&saved, &entries))
        .map_err(|e| invalid(e.to_string()))?
        .hash(&mut hasher);
    let fingerprint = format!("{:016x}", hasher.finish());
    let applied = setting_get(conn, &applied_key(id)).and_then(|v| v.as_str().map(str::to_owned));
    let error = setting_get(conn, &error_key(id)).and_then(|v| v.as_str().map(str::to_owned));
    Ok(Prepared {
        program: StationProgram {
            revision: saved.revision.to_string(),
            items: saved.items,
            sources,
            needs_apply: applied.as_deref() != Some(&fingerprint),
            timezone: std::env::var("TZ").ok().filter(|v| !v.trim().is_empty()),
            issues,
            error,
        },
        entries,
        fingerprint,
    })
}

use rusqlite::OptionalExtension;

pub fn issues_text(program: &StationProgram) -> String {
    let name = |id: &str| {
        program
            .sources
            .iter()
            .find(|s| s.playlist_id == id)
            .map(|s| s.name.as_str())
            .unwrap_or(id)
            .to_owned()
    };
    program
        .issues
        .iter()
        .map(|issue| match issue {
            StationProgramIssue::EmptyQueue => {
                "Add a system playlist or all tracks before broadcasting".into()
            }
            StationProgramIssue::MissingSource { playlist_id } => {
                format!("Playlist missing: {}", name(playlist_id))
            }
            StationProgramIssue::EmptySource { playlist_id } => {
                format!("No playable tracks: {}", name(playlist_id))
            }
            StationProgramIssue::UncoveredTime { weekday, time } => format!(
                "No regular queue for uncovered time: weekday {} {time}",
                weekday + 1
            ),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn slot_minutes(s: &StationSlot) -> DbResult<Vec<usize>> {
    let start = crate::station::hm_minutes(&s.start_hm)
        .ok_or_else(|| invalid("Start time must be HH:MM"))? as usize;
    let end = end_minutes(&s.end_hm).ok_or_else(|| invalid("End time must be HH:MM"))? as usize;
    if start == end || s.days_mask == 0 || s.days_mask > 127 {
        return Err(invalid("Select weekdays and a nonzero time range"));
    }
    let len = if end > start {
        end - start
    } else {
        1440 - start + end
    };
    Ok((0..7)
        .filter(|d| s.days_mask & (1 << d) != 0)
        .flat_map(|d| (0..len).map(move |m| (d * 1440 + start + m) % 10080))
        .collect())
}

fn coverage(items: &[StationQueueItem]) -> DbResult<Vec<bool>> {
    let mut covered = vec![false; 10080];
    for i in items.iter().filter(|i| i.scheduled) {
        for s in &i.slots {
            for m in slot_minutes(s)? {
                covered[m] = true;
            }
        }
    }
    Ok(covered)
}

pub fn save(conn: &mut Connection, id: &str, edit: StationProgramEdit) -> DbResult<()> {
    let tx = conn.transaction()?;
    let current = read(&tx, id)?;
    if current.revision.to_string() != edit.revision {
        return Err(invalid("Configuration changed. Reload before saving."));
    }
    let mut ids = HashSet::new();
    let mut occupied = std::collections::HashMap::new();
    for item in &edit.items {
        if !ids.insert(&item.playlist_id) {
            return Err(invalid("A playlist may only appear once in the queue"));
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id=?1)",
            [&item.playlist_id],
            |r| r.get(0),
        )?;
        if !exists && item.playlist_id != LIBRARY {
            return Err(invalid("Selected playlist no longer exists"));
        }
        if item.scheduled && item.slots.is_empty() {
            return Err(invalid("Select a weekly time range"));
        }
        for slot in &item.slots {
            for minute in slot_minutes(slot)? {
                if item.scheduled
                    && occupied
                        .insert((minute, item.priority), &item.playlist_id)
                        .is_some_and(|owner| owner != &item.playlist_id)
                {
                    return Err(invalid(format!(
                        "Overlapping slots with equal priority at weekday {} {:02}:{:02}",
                        minute / 1440 + 1,
                        minute % 1440 / 60,
                        minute % 60
                    )));
                }
            }
        }
    }
    setting_set(
        &tx,
        &key(id),
        &serde_json::to_value(Saved {
            revision: current.revision + 1,
            items: edit.items,
        })
        .map_err(|e| invalid(e.to_string()))?,
    )?;
    tx.commit()?;
    Ok(())
}

pub fn applied(conn: &Connection, id: &str, fingerprint: &str) -> DbResult<()> {
    setting_set(conn, &applied_key(id), &serde_json::json!(fingerprint))?;
    setting_set(conn, &error_key(id), &serde_json::Value::Null)?;
    Ok(())
}

pub fn failed(conn: &Connection, id: &str, error: &str) -> DbResult<()> {
    setting_set(conn, &error_key(id), &serde_json::json!(error))?;
    setting_set(conn, &applied_key(id), &serde_json::Value::Null)?;
    Ok(())
}

fn liq_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization")
}
fn uri(entry: &Entry, playlist_id: &str) -> String {
    let safe = |v: &str| {
        v.chars()
            .filter(|c| !c.is_control() && *c != '\\')
            .collect::<String>()
            .replace('"', "'")
    };
    format!(
        "annotate:title=\"{}\",aiya_id=\"{}\",aiya_playlist=\"{}\":{}",
        safe(&entry.title),
        safe(&entry.id),
        safe(playlist_id),
        entry.path
    )
}

/// 引擎来源：常规项逐单播放一轮；指定节目按优先级覆盖，空档回到常规来源。
pub fn render(prepared: &Prepared, dir: &str, cursor: Option<&str>) -> (String, String, String) {
    let mut definitions = String::new();
    let mut groups = Vec::new();
    let mut branches = Vec::new();
    for (idx, (item, entries)) in prepared
        .program
        .items
        .iter()
        .zip(&prepared.entries)
        .enumerate()
    {
        let tracks = format!(
            "[{}]",
            entries
                .iter()
                .map(|e| liq_string(&uri(e, &item.playlist_id)))
                .collect::<Vec<_>>()
                .join(",")
        );
        if item.scheduled {
            let mode = if item.play_mode == StationPlayMode::Shuffle {
                "randomize"
            } else {
                "normal"
            };
            definitions +=
                &format!("pl{idx} = playlist.list(id=\"pl{idx}\", mode=\"{mode}\", {tracks})\n");
            let mut rules = Vec::new();
            for slot in &item.slots {
                let start = crate::station::hm_minutes(&slot.start_hm).unwrap_or(0);
                let end = end_minutes(&slot.end_hm).unwrap_or(0);
                for day in 0..7 {
                    if slot.days_mask & (1 << day) == 0 {
                        continue;
                    }
                    let t = |m: u32| format!("{}h{}m", m / 60, m % 60);
                    if start < end {
                        rules.push(format!("({}-{} and {}w)", t(start), t(end), day + 1));
                    } else {
                        rules.push(format!("({}-24h and {}w)", t(start), day + 1));
                        if end > 0 {
                            rules.push(format!("(0h-{} and {}w)", t(end), (day + 1) % 7 + 1));
                        }
                    }
                }
            }
            if !rules.is_empty() {
                branches.push((
                    item.priority,
                    format!("({{{}}},pl{idx})", rules.join(" or ")),
                ));
            }
        } else {
            groups.push(format!(
                "({tracks},{})",
                item.play_mode == StationPlayMode::Shuffle
            ));
        }
    }
    let base = if groups.is_empty() {
        "blank(duration=0.)".into()
    } else {
        let regular: Vec<_> = prepared
            .program
            .items
            .iter()
            .zip(&prepared.entries)
            .filter(|(i, _)| !i.scheduled)
            .collect();
        let resume = cursor.and_then(|cursor| {
            let (playlist_id, id) = cursor.split_once('\t')?;
            regular.iter().enumerate().find_map(|(g, (item, list))| {
                (item.playlist_id == playlist_id)
                    .then(|| list.iter().position(|e| e.id == id).map(|n| (g, n)))
                    .flatten()
            })
        });
        let (next_group, pending) = match resume {
            Some((g, n)) => (
                (g + 1) % groups.len(),
                regular[g]
                    .1
                    .iter()
                    .skip(n + 1)
                    .map(|e| liq_string(&uri(e, &regular[g].0.playlist_id)))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            None => (0, String::new()),
        };
        definitions += &format!(
            r#"regular_groups = [{}]
regular_group = ref({})
regular_pending = ref([{}])
def regular_next() =
  if list.is_empty(regular_pending()) then
    group = list.nth(regular_groups, regular_group())
    tracks = fst(group)
    tracks = if snd(group) then list.shuffle(tracks) else tracks end
    regular_pending.set(tracks)
    regular_group.set((regular_group() + 1) mod list.length(regular_groups))
  end
  next = list.hd(regular_pending())
  regular_pending.set(list.tl(regular_pending()))
  request.create(next)
end
regular = request.dynamic(id="regular", prefetch=1, synchronous=true, regular_next)
regular.on_track(synchronous=true, fun (m) -> file.write(data=(m["aiya_playlist"] ?? "") ^ "\t" ^ (m["aiya_id"] ?? ""), append=false, {}))
"#,
            groups.join(","),
            next_group,
            pending,
            liq_string(&format!("{dir}/regular.cursor"))
        );
        "regular".into()
    };
    branches.sort_by_key(|a| std::cmp::Reverse(a.0));
    let schedule = if branches.is_empty() {
        String::new()
    } else {
        let transitions = vec!["advance_program"; branches.len() + 1].join(",");
        format!(
            "def advance_program(previous, next) =\n  source.skip(previous)\n  next\nend\nsch = switch(track_sensitive=false,transition_length=0.,transitions=[{transitions}],[{},({{true}},{base})])",
            branches
                .into_iter()
                .map(|(_, s)| s)
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    (definitions, base, schedule)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> Connection {
        let mut c = aiya_data::db::open_in_memory().unwrap();
        c.execute_batch("INSERT INTO tracks (id,title,title_norm,path,path_lower,added_at) VALUES ('a','A','a','/music/a.mp3','/music/a.mp3',0),('b','B','b','/music/b.mp3','/music/b.mp3',0)").unwrap();
        aiya_data::playlist::create(&mut c, "p", "Playlist", &["a".into(), "b".into()], 0).unwrap();
        c
    }
    fn item() -> StationQueueItem {
        StationQueueItem {
            playlist_id: "p".into(),
            play_mode: StationPlayMode::Ordered,
            priority: 0,
            scheduled: false,
            slots: vec![],
        }
    }
    fn slot(start: &str, end: &str, days: u8) -> StationSlot {
        StationSlot {
            start_hm: start.into(),
            end_hm: end.into(),
            days_mask: days,
        }
    }

    #[test]
    fn references_follow_system_members_names_and_deletion_without_copies() {
        let mut c = setup();
        save(
            &mut c,
            "s",
            StationProgramEdit {
                revision: "0".into(),
                items: vec![item()],
            },
        )
        .unwrap();
        let before = prepare(&c, "s").unwrap();
        assert_eq!(
            before.entries[0]
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        applied(&c, "s", &before.fingerprint).unwrap();
        assert!(!prepare(&c, "s").unwrap().program.needs_apply);
        aiya_data::playlist::rename(&c, "p", "Renamed", 1).unwrap();
        assert_eq!(prepare(&c, "s").unwrap().program.sources[1].name, "Renamed");
        aiya_data::playlist::remove_at(&mut c, "p", 0, 2).unwrap();
        let after = prepare(&c, "s").unwrap();
        assert_eq!(after.entries[0][0].id, "b");
        assert!(after.program.needs_apply);
        aiya_data::playlist::delete(&c, "p").unwrap();
        let missing = prepare(&c, "s").unwrap();
        assert!(!missing.program.sources[1].exists);
        assert!(!missing.program.issues.is_empty());
    }

    #[test]
    fn slot_minutes_keep_partial_hours_and_sunday_overnight() {
        let minutes = slot_minutes(&slot("22:30", "02:15", 64)).unwrap();
        assert!(minutes.contains(&(6 * 1440 + 1350)));
        assert!(minutes.contains(&134));
        assert!(!minutes.contains(&135));
        assert_eq!(slot_minutes(&slot("08:15", "08:45", 1)).unwrap().len(), 30);
        for s in [
            slot("24:00", "01:00", 1),
            slot("08:00", "08:00", 1),
            slot("08:00", "09:00", 0),
        ] {
            assert!(slot_minutes(&s).is_err());
        }
    }

    #[test]
    fn conflicting_edits_rollback_and_new_renderer_has_a_real_regular_branch() {
        let mut c = setup();
        save(
            &mut c,
            "s",
            StationProgramEdit {
                revision: "0".into(),
                items: vec![item()],
            },
        )
        .unwrap();
        assert!(save(
            &mut c,
            "s",
            StationProgramEdit {
                revision: "0".into(),
                items: vec![]
            }
        )
        .is_err());
        let mut show = item();
        show.scheduled = true;
        show.slots = vec![slot("22:30", "02:15", 64), slot("01:00", "03:00", 1)];
        assert_eq!(prepare(&c, "s").unwrap().program.revision, "1");
        aiya_data::playlist::create(&mut c, "q", "Second", &["a".into()], 0).unwrap();
        let mut second = show.clone();
        second.playlist_id = "q".into();
        show.slots.pop();
        second.slots.remove(0);
        assert!(save(
            &mut c,
            "s",
            StationProgramEdit {
                revision: "1".into(),
                items: vec![show.clone(), second.clone()]
            }
        )
        .is_err());
        second.priority = 1;
        save(
            &mut c,
            "s",
            StationProgramEdit {
                revision: "1".into(),
                items: vec![show, second],
            },
        )
        .unwrap();
        assert!(prepare(&c, "s")
            .unwrap()
            .program
            .issues
            .iter()
            .any(|v| matches!(v, StationProgramIssue::UncoveredTime { .. })));
        let mut prepared = prepare(&c, "s").unwrap();
        prepared.program.items.push(item());
        prepared.entries.push(prepared.entries[0].clone());
        let (defs, base, schedule) = render(&prepared, "/hls/ch1", Some("p\ta"));
        assert_eq!(base, "regular");
        assert!(defs.contains("regular_pending = ref([\"annotate:title=\\\"B"));
        assert!(defs.contains("prefetch=1"));
        assert!(schedule.contains("22h30m-24h and 7w"));
        assert!(schedule.contains("0h-2h15m and 1w"));
        assert!(schedule.contains("({true},regular)"));
        assert!(schedule.contains("source.skip(previous)"));
    }

    #[test]
    fn library_source_works_without_playlists_and_tracks_live_membership() {
        let mut c = setup();
        aiya_data::playlist::delete(&c, "p").unwrap();
        let mut all = item();
        all.playlist_id = LIBRARY.into();
        save(
            &mut c,
            "s",
            StationProgramEdit {
                revision: "0".into(),
                items: vec![all.clone()],
            },
        )
        .unwrap();
        let before = prepare(&c, "s").unwrap();
        assert!(before.program.issues.is_empty());
        assert_eq!(before.program.sources.len(), 1);
        assert_eq!(before.program.sources[0].track_count, 2);
        assert_eq!(before.entries[0].len(), 2);
        applied(&c, "s", &before.fingerprint).unwrap();
        c.execute("UPDATE tracks SET availability='offline' WHERE id='b'", [])
            .unwrap();
        let after = prepare(&c, "s").unwrap();
        assert!(after.program.needs_apply);
        assert_eq!(after.program.sources[0].available_count, 1);
        assert_eq!(after.entries[0][0].id, "a");
        all.scheduled = true;
        all.priority = 7;
        all.slots = vec![slot("00:00", "24:00", 127), slot("08:30", "09:00", 1)];
        save(
            &mut c,
            "s",
            StationProgramEdit {
                revision: "1".into(),
                items: vec![all],
            },
        )
        .unwrap();
        let full = prepare(&c, "s").unwrap();
        assert!(full.program.issues.is_empty());
        assert_eq!(
            slot_minutes(&full.program.items[0].slots[0]).unwrap().len(),
            10080
        );
        assert!(render(&full, "/hls/ch1", None)
            .2
            .contains("0h0m-24h0m and 7w"));
    }

    #[test]
    fn resume_uses_playlist_identity_when_tracks_appear_in_multiple_groups() {
        let mut c = setup();
        aiya_data::playlist::create(&mut c, "q", "Second", &["a".into(), "b".into()], 0).unwrap();
        let mut second = item();
        second.playlist_id = "q".into();
        save(
            &mut c,
            "s",
            StationProgramEdit {
                revision: "0".into(),
                items: vec![item(), second],
            },
        )
        .unwrap();
        let prepared = prepare(&c, "s").unwrap();
        let (defs, _, _) = render(&prepared, "/hls/ch1", Some("q\ta"));
        assert!(defs.contains("regular_group = ref(0)"));
        assert!(defs.contains("regular_pending = ref([\"annotate:title=\\\"B"));
        assert!(defs.contains("aiya_playlist=\\\"q\\\""));
    }
}
