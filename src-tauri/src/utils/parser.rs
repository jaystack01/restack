use crate::models::ParsedEpisode;
use regex::Regex;
use std::sync::LazyLock;

// Regexes are compiled once on first use; parsing runs several times per scanned file.
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(1080p|720p|2160p|4k|480p|x264|x265|h\.?264|h\.?265|hevc|avc|hindi|english|dual\.?audio|web[-\s]?dl|webrip|bluray|brrip|dvdrip|hdrip|hdtv|remux|proper|repack|extended|unrated|directors\.?cut|theatrical)\b"
    ).expect("valid tag regex")
});
static BRACKET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[.*?\]|\(.*?\)").expect("valid bracket regex"));
static PUNCTUATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[._-]+").expect("valid punctuation regex"));
static SPACES_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid spaces regex"));

// Movie with year: "Inception 2010", "The Dark Knight 2008"
static MOVIE_YEAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.+?)\s+((?:19|20)\d{2})\b\s*(.*)$").expect("valid movie year regex")
});

// Standard series patterns with explicit season + episode, in priority order
static SERIES_RES: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)^(.+?)\s*S(\d+)E(\d+)(?:\s+(.+))?$").expect("valid primary regex"),
        Regex::new(r"(?i)^(.+?)\s*(\d+)x(\d+)(?:\s+(.+))?$").expect("valid fallback regex"),
        Regex::new(r"(?i)^(.+?)\s*Season\s*(\d+)\s*Episode\s*(\d+)(?:\s+(.+))?$").expect("valid fallback regex"),
    ]
});

static E_ONLY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(.+?)\s+E(\d{1,3})(?:\s+(.+))?$").expect("valid E-only regex"));
static LIGHT_CLEAN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[._]+").expect("valid"));
static DASH_EP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(.+?)\s*-\s*(\d{1,3})(?:\s+(.+))?$").expect("valid dash-ep regex")
});

pub fn clean_text(raw: &str) -> String {
    let step1 = BRACKET_RE.replace_all(raw, " ");
    let step2 = PUNCTUATION_RE.replace_all(&step1, " ");
    let step3 = TAG_RE.replace_all(&step2, " ");
    let step4 = SPACES_RE.replace_all(&step3, " ");

    step4.trim().to_string()
}

/// Result of parsing a filename — every video gets classified.
pub enum ParsedMedia {
    /// Matched SxxExx / NxNN / Season X Episode Y
    Series(ParsedEpisode),
    /// Looks like a movie (name + optional year, no episode pattern)
    Movie { title: String, year: Option<u32> },
    /// Truly unidentifiable — use raw filename
    Other { raw_name: String },
}

/// Try to parse as a series first, then as a movie, then fall back to Other.
pub fn parse_filename(stem: &str) -> ParsedMedia {
    // --- Try series patterns first ---
    if let Some(parsed) = try_parse_series(stem) {
        return ParsedMedia::Series(parsed);
    }

    // --- Try movie patterns ---
    let cleaned = clean_text(stem);
    if cleaned.is_empty() {
        return ParsedMedia::Other { raw_name: stem.to_string() };
    }

    // Movie with year: "Inception 2010", "The Dark Knight 2008"
    if let Some(caps) = MOVIE_YEAR_RE.captures(&cleaned) {
        let title = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("").to_string();
        let year: Option<u32> = caps.get(2).and_then(|m| m.as_str().parse().ok());
        if !title.is_empty() {
            return ParsedMedia::Movie { title, year };
        }
    }

    ParsedMedia::Other { raw_name: stem.to_string() }
}

/// Series parsing logic — returns None if no series pattern matches.
fn try_parse_series(stem: &str) -> Option<ParsedEpisode> {
    let cleaned = clean_text(stem);

    // --- Standard patterns with explicit season + episode ---
    for pattern in SERIES_RES.iter() {
        if let Some(caps) = pattern.captures(&cleaned) {
            let show_name = clean_text(caps.get(1)?.as_str());
            let season = caps.get(2)?.as_str().parse::<u32>().ok()?;
            let episode = caps.get(3)?.as_str().parse::<u32>().ok()?;

            let title = caps
                .get(4)
                .map(|m| clean_text(m.as_str()))
                .filter(|v| !v.is_empty());

            if !show_name.is_empty() {
                return Some(ParsedEpisode {
                    show_name,
                    season,
                    episode,
                    title,
                });
            }
        }
    }

    // --- Fallback: episode-only patterns (no season → default season 1) ---

    // Pattern on cleaned text: "ShowName E04 OptionalTitle"
    if let Some(caps) = E_ONLY_RE.captures(&cleaned) {
        let show_name = clean_text(caps.get(1)?.as_str());
        let episode = caps.get(2)?.as_str().parse::<u32>().ok()?;
        let title = caps.get(3).map(|m| clean_text(m.as_str())).filter(|v| !v.is_empty());
        if !show_name.is_empty() && episode > 0 {
            return Some(ParsedEpisode { show_name, season: 1, episode, title });
        }
    }

    // Dash-episode pattern on original stem (anime convention: "Name-04" or "Name - 04")
    // Only replace dots/underscores with spaces, keep dashes intact for detection
    let light_clean = LIGHT_CLEAN_RE.replace_all(stem, " ");
    if let Some(caps) = DASH_EP_RE.captures(&light_clean) {
        let raw_name = caps.get(1)?.as_str();
        let show_name = clean_text(raw_name);
        let episode = caps.get(2)?.as_str().parse::<u32>().ok()?;
        let title = caps.get(3).map(|m| clean_text(m.as_str())).filter(|v| !v.is_empty());
        if !show_name.is_empty() && episode > 0 {
            return Some(ParsedEpisode { show_name, season: 1, episode, title });
        }
    }

    None
}
