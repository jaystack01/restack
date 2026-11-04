use crate::models::ParsedEpisode;
use regex::Regex;
use std::sync::LazyLock;

// Regexes are compiled once on first use; parsing runs several times per scanned file.
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(\d{3,4}x\d{3,4}|1080p|720p|2160p|4k|480p|x264|x265|h\.?264|h\.?265|hevc|avc|hindi|english|dual\.?audio|web[-\s]?dl|webrip|bluray|brrip|dvdrip|hdrip|hdtv|remux|proper|repack|extended|unrated|directors\.?cut|theatrical)\b"
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

// Standard series patterns with explicit season + episode, in priority order.
// NxNN is bounded (season 1-2 digits, episode 1-3) so resolutions like 1920x1080 never match.
static SERIES_RES: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)^(.+?)\s*S(\d+)E(\d+)(?:\s+(.+))?$").expect("valid primary regex"),
        Regex::new(r"(?i)^(.+?)\s*\b(\d{1,2})x(\d{1,3})(?:\s+(.+))?$").expect("valid fallback regex"),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Comparable view of `ParsedMedia` (which has no Debug/PartialEq).
    #[derive(Debug, PartialEq)]
    enum Got {
        Series(String, u32, u32, Option<String>),
        Movie(String, Option<u32>),
        Other(String),
    }

    fn parse(stem: &str) -> Got {
        match parse_filename(stem) {
            ParsedMedia::Series(p) => Got::Series(p.show_name, p.season, p.episode, p.title),
            ParsedMedia::Movie { title, year } => Got::Movie(title, year),
            ParsedMedia::Other { raw_name } => Got::Other(raw_name),
        }
    }

    fn series(show: &str, season: u32, episode: u32, title: Option<&str>) -> Got {
        Got::Series(show.into(), season, episode, title.map(Into::into))
    }

    fn movie(title: &str, year: u32) -> Got {
        Got::Movie(title.into(), Some(year))
    }

    fn other(raw: &str) -> Got {
        Got::Other(raw.into())
    }

    fn check(cases: &[(&str, Got)]) {
        for (stem, want) in cases {
            assert_eq!(&parse(stem), want, "stem: {stem:?}");
        }
    }

    #[test]
    fn clean_text_strips_brackets_punctuation_and_tags() {
        assert_eq!(clean_text("[Group] Show.Name_(2020)-1080p.WEB-DL.x265"), "Show Name");
        assert_eq!(clean_text("  a..b   c "), "a b c");
        assert_eq!(clean_text("[only tags] 1080p"), "");
    }

    #[test]
    fn parses_sxxexx() {
        check(&[
            ("Breaking.Bad.S01E02.Gray.Matter.1080p.WEB-DL.x264", series("Breaking Bad", 1, 2, Some("Gray Matter"))),
            ("the.office.s03e10", series("the office", 3, 10, None)),
            ("Show Name S10E100", series("Show Name", 10, 100, None)),
            ("[Group] Show_Name_S02E05_[1080p]", series("Show Name", 2, 5, None)),
        ]);
    }

    #[test]
    fn parses_nxnn() {
        check(&[
            ("Friends 1x05 The One Where", series("Friends", 1, 5, Some("The One Where"))),
            ("The.Simpsons.10x05", series("The Simpsons", 10, 5, None)),
        ]);
    }

    #[test]
    fn parses_season_episode_words() {
        check(&[
            ("Show Name Season 2 Episode 3", series("Show Name", 2, 3, None)),
            ("Show.Name.Season.1.Episode.12.Pilot", series("Show Name", 1, 12, Some("Pilot"))),
        ]);
    }

    #[test]
    fn parses_episode_only_as_season_one() {
        check(&[
            ("Naruto E04 The Title", series("Naruto", 1, 4, Some("The Title"))),
            ("Some.Show.E12", series("Some Show", 1, 12, None)),
        ]);
    }

    #[test]
    fn parses_anime_dash_episode_as_season_one() {
        check(&[
            ("[SubsPlease] Frieren - 04 (1080p) [ABCD1234]", series("Frieren", 1, 4, None)),
            ("Show-07", series("Show", 1, 7, None)),
            ("Some_Anime - 12 Finale", series("Some Anime", 1, 12, Some("Finale"))),
        ]);
    }

    #[test]
    fn parses_movies_with_year() {
        check(&[
            ("Inception.2010.1080p.BluRay.x264", movie("Inception", 2010)),
            ("The Dark Knight 2008", movie("The Dark Knight", 2008)),
            ("Some_Movie_1999_REMUX", movie("Some Movie", 1999)),
        ]);
    }

    #[test]
    fn titles_that_are_years_keep_their_name() {
        check(&[
            ("1917.2019.1080p.BluRay", movie("1917", 2019)),
            ("2012.2009.720p", movie("2012", 2009)),
            ("1923.S01E01", series("1923", 1, 1, None)),
            // A bare year has no separate title, so it can't be told apart from a year tag.
            ("1917", other("1917")),
        ]);
    }

    #[test]
    fn falls_back_to_other_with_raw_stem() {
        check(&[
            ("random_clip", other("random_clip")),
            ("VID_20240514_123045", other("VID_20240514_123045")),
            ("[1080p]", other("[1080p]")),
            ("", other("")),
        ]);
    }

    #[test]
    fn bug15_resolution_is_not_an_episode() {
        check(&[
            ("Inception.2010.1920x1080", movie("Inception", 2010)),
            ("Home.Video.1280x720", other("Home.Video.1280x720")),
            ("Some.Movie.2019.3840X2160.HDR", movie("Some Movie", 2019)),
            ("Show.S01E02.1920x1080", series("Show", 1, 2, None)),
            // Real NxNN episodes still parse.
            ("Show.1x5", series("Show", 1, 5, None)),
            ("Show 12x103 Title", series("Show", 12, 103, Some("Title"))),
        ]);
    }

    #[test]
    #[ignore = "BUG-16: date/time-stamped recordings parse as series"]
    fn bug16_timestamped_recordings_are_not_series() {
        for stem in ["Screencast from 2024-05-14 12-30-45", "OBS 2024-05-14 10-30-12"] {
            let got = parse(stem);
            assert!(!matches!(got, Got::Series(..)), "stem: {stem:?} parsed as {got:?}");
        }
    }

    #[test]
    #[ignore = "BUG-17: multi-episode, year-suffixed and split S/E releases misparse"]
    fn bug17_multi_episode_and_year_suffixed_series() {
        check(&[
            ("Breaking.Bad.S01E01E02", series("Breaking Bad", 1, 1, None)),
            ("Doctor.Who.2005.S01E01", series("Doctor Who", 1, 1, None)),
            ("show.s01.e02", series("show", 1, 2, None)),
        ]);
    }

    #[test]
    #[ignore = "BUG-18: movie year takes the first year-like number"]
    fn bug18_movie_year_prefers_release_year() {
        check(&[
            ("Blade Runner 2049 (2017)", movie("Blade Runner 2049", 2017)),
            ("Blade.Runner.2049.2017.1080p", movie("Blade Runner 2049", 2017)),
        ]);
    }
}
