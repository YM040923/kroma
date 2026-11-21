use anyhow::Result;
use std::collections::HashMap;
use tantivy::schema::Schema;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy};

use super::document::Indexer;
use super::entry::Entry;
use super::hit::HitKind;
use super::people::Person;
use super::schema::{self, Fields, ANALYZER};
use crate::db::translations::TransData;
use crate::model::{MediaItem, Show};

pub(super) struct Active {
    pub(super) reader: IndexReader,
    // Held so the in-RAM directory and its registered tokenizer outlive `reader`.
    pub(super) _index: Index,
    pub(super) entries: Vec<Entry>,
    pub(super) people: Vec<Person>,
    pub(super) libraries: Vec<String>,
}

pub(super) struct Catalog<'a> {
    pub movies: &'a [MediaItem],
    pub shows: &'a [Show],
    pub episodes: &'a [MediaItem],
    pub tr_movies: &'a HashMap<String, Vec<TransData>>,
    pub tr_episodes: &'a HashMap<String, Vec<TransData>>,
    pub tr_shows: &'a HashMap<String, Vec<TransData>>,
}

pub(super) fn build_active(schema: Schema, fields: &Fields, catalog: &Catalog) -> Result<Active> {
    let index = schema::new_index(schema);
    let mut writer: IndexWriter = index.writer_with_num_threads(1, 15_000_000)?;
    let mut indexer = Indexer::new(&mut writer, fields, schema::analyzer());
    for movie in catalog.movies {
        indexer.add_item(movie, HitKind::Movie, catalog.tr_movies.get(&movie.id));
    }
    for episode in catalog.episodes {
        indexer.add_item(
            episode,
            HitKind::Episode,
            catalog.tr_episodes.get(&episode.id),
        );
    }
    for show in catalog.shows {
        indexer.add_show(show, catalog.tr_shows.get(&show.id));
    }
    let (entries, people) = indexer.finish();
    let libraries: std::collections::BTreeSet<String> =
        entries.iter().map(|entry| entry.library.clone()).collect();
    writer.commit()?;
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    reader.reload()?;
    Ok(Active {
        reader,
        _index: index,
        entries,
        people,
        libraries: libraries.into_iter().collect(),
    })
}

// Uses the index's own analyzer so query terms fold identically to indexed ones.
pub(super) fn normalize(index: &Index, raw: &str) -> Vec<String> {
    let Some(mut analyzer) = index.tokenizers().get(ANALYZER) else {
        return raw.split_whitespace().map(str::to_lowercase).collect();
    };
    super::entry::tokens(&mut analyzer, raw)
        .into_iter()
        .flat_map(split_scripts)
        .collect()
}

/// `liulang地球` reaches the analyzer as one token — every character in it is
/// alphanumeric. To a reader it is two words, so it is split where one script
/// gives way to the other; digits stay with the run they follow.
fn split_scripts(token: String) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut latin_run: Option<bool> = None;
    for c in token.chars() {
        let run = if c.is_ascii_alphabetic() {
            Some(true)
        } else if super::pinyin::is_cjk(c) {
            Some(false)
        } else {
            None
        };
        if let Some(kind) = run {
            if latin_run.is_some_and(|current| current != kind) {
                out.push(std::mem::take(&mut current));
            }
            latin_run = Some(kind);
        }
        current.push(c);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}
