//! In-memory library: lookup, column-browser lists, filtering, sorting, art.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use lofty::prelude::*;

use crate::track::{Track, TrackId};

/// Narrowing criteria for [`Library::query`]. `None` or empty means "all".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Exact genre match.
    pub genre: Option<String>,
    /// Exact artist match.
    pub artist: Option<String>,
    /// Exact album match.
    pub album: Option<String>,
    /// Case-insensitive substring over title, artist and album.
    pub search: String,
}

/// Column a track list is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    /// Title, then artist.
    Title,
    /// Artist, then album, disc, track, title.
    Artist,
    /// Album, then disc, track, title. This is the default ordering.
    Album,
    /// Genre, then the album ordering.
    Genre,
    /// Duration, then title.
    Duration,
    /// Track number, then title.
    TrackNo,
}

/// Column plus direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    /// Column to order by.
    pub column: SortColumn,
    /// `true` for ascending.
    pub ascending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            column: SortColumn::Album,
            ascending: true,
        }
    }
}

/// All scanned tracks plus derived lists and a cover-art cache.
pub struct Library {
    tracks: Vec<Track>,
    genres: Vec<String>,
    art_cache: Mutex<HashMap<TrackId, Option<Arc<[u8]>>>>,
}

#[allow(clippy::missing_fields_in_debug)] // Summarised on purpose: tracks print as a count, the cache is omitted.
impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library")
            .field("tracks", &self.tracks.len())
            .finish()
    }
}

impl Library {
    /// Build a library, assigning sequential ids in the given order.
    pub fn new(mut tracks: Vec<Track>) -> Self {
        for (index, track) in tracks.iter_mut().enumerate() {
            track.id = TrackId(u32::try_from(index).unwrap_or(u32::MAX));
        }
        let genres = unique_sorted(tracks.iter().map(|t| t.genre.as_str()));
        Self {
            tracks,
            genres,
            art_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Number of tracks.
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Whether the library has no tracks.
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Every track in id order.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Look up one track.
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(id.0 as usize)
    }

    /// Distinct genres, sorted case-insensitively.
    pub fn genres(&self) -> &[String] {
        &self.genres
    }

    /// Distinct artists, optionally restricted to one genre.
    pub fn artists_in(&self, genre: Option<&str>) -> Vec<String> {
        unique_sorted(
            self.tracks
                .iter()
                .filter(|t| genre.is_none_or(|g| t.genre == g))
                .map(|t| t.artist.as_str()),
        )
    }

    /// Distinct albums, optionally restricted to one genre and/or artist.
    pub fn albums_in(&self, genre: Option<&str>, artist: Option<&str>) -> Vec<String> {
        unique_sorted(
            self.tracks
                .iter()
                .filter(|t| genre.is_none_or(|g| t.genre == g))
                .filter(|t| artist.is_none_or(|a| t.artist == a))
                .map(|t| t.album.as_str()),
        )
    }

    /// Ids of the tracks matching `filter`, ordered by `sort`.
    pub fn query(&self, filter: &Filter, sort: Sort) -> Vec<TrackId> {
        let needle = filter.search.trim().to_lowercase();
        let mut ids: Vec<TrackId> = self
            .tracks
            .iter()
            .filter(|t| filter.genre.as_deref().is_none_or(|g| t.genre == g))
            .filter(|t| filter.artist.as_deref().is_none_or(|a| t.artist == a))
            .filter(|t| filter.album.as_deref().is_none_or(|a| t.album == a))
            .filter(|t| needle.is_empty() || matches_search(t, &needle))
            .map(|t| t.id)
            .collect();
        self.sort_ids(&mut ids, sort);
        ids
    }

    /// Reorder `ids` in place by `sort`. Unknown ids sort last.
    pub fn sort_ids(&self, ids: &mut [TrackId], sort: Sort) {
        ids.sort_by(|a, b| {
            let ordering = match (self.track(*a), self.track(*b)) {
                (Some(a), Some(b)) => compare(a, b, sort.column),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            };
            if sort.ascending {
                ordering
            } else {
                ordering.reverse()
            }
        });
    }

    /// Raw bytes of the embedded front cover, read lazily and memoized.
    pub fn cover_art(&self, id: TrackId) -> Option<Arc<[u8]>> {
        let track = self.track(id)?;
        if !track.has_art {
            return None;
        }
        let mut cache = self
            .art_cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        cache.entry(id).or_insert_with(|| read_art(track)).clone()
    }
}

fn read_art(track: &Track) -> Option<Arc<[u8]>> {
    let tagged = lofty::read_from_path(&track.path)
        .map_err(|err| tracing::warn!(path = %track.path.display(), %err, "cover art unreadable"))
        .ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let picture = tag.pictures().first()?;
    Some(Arc::from(picture.data()))
}

fn matches_search(track: &Track, needle: &str) -> bool {
    track.title.to_lowercase().contains(needle)
        || track.artist.to_lowercase().contains(needle)
        || track.album.to_lowercase().contains(needle)
}

fn unique_sorted<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = values.map(str::to_owned).collect();
    out.sort_by(|a, b| ci(a, b).then_with(|| b.cmp(a)));
    out.dedup();
    out
}

fn ci(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

fn album_order(a: &Track, b: &Track) -> Ordering {
    ci(&a.album, &b.album)
        .then_with(|| a.disc_no.unwrap_or(0).cmp(&b.disc_no.unwrap_or(0)))
        .then_with(|| a.track_no.unwrap_or(0).cmp(&b.track_no.unwrap_or(0)))
        .then_with(|| ci(&a.title, &b.title))
}

fn compare(a: &Track, b: &Track, column: SortColumn) -> Ordering {
    match column {
        SortColumn::Title => ci(&a.title, &b.title).then_with(|| ci(&a.artist, &b.artist)),
        SortColumn::Artist => ci(&a.artist, &b.artist).then_with(|| album_order(a, b)),
        SortColumn::Album => album_order(a, b),
        SortColumn::Genre => ci(&a.genre, &b.genre).then_with(|| album_order(a, b)),
        SortColumn::Duration => a
            .duration
            .cmp(&b.duration)
            .then_with(|| ci(&a.title, &b.title)),
        SortColumn::TrackNo => a
            .track_no
            .unwrap_or(0)
            .cmp(&b.track_no.unwrap_or(0))
            .then_with(|| ci(&a.title, &b.title)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn track(
        title: &str,
        artist: &str,
        album: &str,
        genre: &str,
        disc: u32,
        no: u32,
        secs: u64,
    ) -> Track {
        Track {
            id: TrackId(0),
            path: PathBuf::from(format!("/music/{title}.m4a")),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            genre: genre.into(),
            track_no: Some(no),
            disc_no: Some(disc),
            year: None,
            duration: Duration::from_secs(secs),
            has_art: false,
        }
    }

    fn library() -> Library {
        Library::new(vec![
            track("Zeta", "bou", "Swerve It", "Drum & Bass", 1, 2, 200),
            track("Alpha", "Bou", "Swerve It", "Drum & Bass", 1, 1, 180),
            track("Mid", "Shakewell", "Big Juice", "Hip-Hop", 1, 1, 150),
            track("Late", "Shakewell", "Big Juice", "Hip-Hop", 2, 1, 160),
            track("Solo", "Shakewell", "Other", "Hip-Hop", 1, 1, 90),
        ])
    }

    fn titles(lib: &Library, ids: &[TrackId]) -> Vec<String> {
        ids.iter()
            .map(|id| lib.track(*id).unwrap().title.clone())
            .collect()
    }

    #[test]
    fn assigns_sequential_ids() {
        let lib = library();
        let ids: Vec<u32> = lib.tracks().iter().map(|t| t.id.0).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);
        assert_eq!(lib.track(TrackId(2)).unwrap().title, "Mid");
        assert!(lib.track(TrackId(99)).is_none());
    }

    #[test]
    fn lists_are_sorted_case_insensitively_and_deduplicated() {
        let lib = library();
        assert_eq!(lib.genres(), &["Drum & Bass", "Hip-Hop"]);
        // "bou" and "Bou" are distinct artists but sort together.
        assert_eq!(lib.artists_in(None), vec!["bou", "Bou", "Shakewell"]);
        assert_eq!(lib.artists_in(Some("Hip-Hop")), vec!["Shakewell"]);
        assert_eq!(
            lib.albums_in(Some("Hip-Hop"), Some("Shakewell")),
            vec!["Big Juice", "Other"]
        );
        assert_eq!(
            lib.albums_in(None, None),
            vec!["Big Juice", "Other", "Swerve It"]
        );
    }

    #[test]
    fn default_sort_is_album_disc_track_title() {
        let lib = library();
        let ids = lib.query(&Filter::default(), Sort::default());
        assert_eq!(
            titles(&lib, &ids),
            vec!["Mid", "Late", "Solo", "Alpha", "Zeta"]
        );
    }

    #[test]
    fn filters_combine_and_search_is_case_insensitive() {
        let lib = library();
        let filter = Filter {
            genre: Some("Hip-Hop".into()),
            album: Some("Big Juice".into()),
            ..Filter::default()
        };
        assert_eq!(
            titles(&lib, &lib.query(&filter, Sort::default())),
            vec!["Mid", "Late"]
        );

        let filter = Filter {
            search: "sWERVE".into(),
            ..Filter::default()
        };
        assert_eq!(
            titles(&lib, &lib.query(&filter, Sort::default())),
            vec!["Alpha", "Zeta"]
        );

        let filter = Filter {
            search: "nothing here".into(),
            ..Filter::default()
        };
        assert!(lib.query(&filter, Sort::default()).is_empty());
    }

    #[test]
    fn sorts_by_column_in_both_directions() {
        let lib = library();
        let asc = Sort {
            column: SortColumn::Title,
            ascending: true,
        };
        let desc = Sort {
            column: SortColumn::Duration,
            ascending: false,
        };
        assert_eq!(
            titles(&lib, &lib.query(&Filter::default(), asc)),
            vec!["Alpha", "Late", "Mid", "Solo", "Zeta"]
        );
        assert_eq!(
            titles(&lib, &lib.query(&Filter::default(), desc)),
            vec!["Zeta", "Alpha", "Late", "Mid", "Solo"]
        );

        let mut ids = vec![TrackId(0), TrackId(1)];
        lib.sort_ids(&mut ids, asc);
        assert_eq!(titles(&lib, &ids), vec!["Alpha", "Zeta"]);
    }

    #[test]
    fn cover_art_is_none_for_tracks_without_art() {
        let lib = Library::new(vec![track("x", "a", "b", "c", 1, 1, 1)]);
        assert!(lib.cover_art(TrackId(0)).is_none());
        assert!(lib.cover_art(TrackId(5)).is_none());
    }
}
