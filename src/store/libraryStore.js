let cachedLibrary = null; // { series: [], movies: [], others: [] }
let listeners = [];

let playingFilePath = null;
let playingListeners = [];

export function getLibrary() {
  return cachedLibrary;
}

export function getSeries() {
  return cachedLibrary?.series || [];
}

export function getMovies() {
  return cachedLibrary?.movies || [];
}

export function getOthers() {
  return cachedLibrary?.others || [];
}

export function setLibrary(data) {
  cachedLibrary = data;
  listeners.forEach((l) => l(data));
}

// Copy of `items` with the matching entry updated, the same array if it
// already has these values, or null if nothing matches.
function patchItems(items, filePath, lastPosition, watched) {
  const i = items.findIndex((item) => item.filePath === filePath);
  if (i === -1) return null;
  const item = items[i];
  if (item.lastPosition === lastPosition && item.watched === watched)
    return items;
  const next = items.slice();
  next[i] = { ...item, lastPosition, watched };
  return next;
}

// Only the path to the matching item is rebuilt; untouched shows, seasons and
// items keep their identity. Listeners aren't notified if nothing changed.
function patchLibrary(library, filePath, lastPosition, watched) {
  for (let s = 0; s < library.series.length; s++) {
    const show = library.series[s];
    for (let k = 0; k < show.seasons.length; k++) {
      const season = show.seasons[k];
      const episodes = patchItems(
        season.episodes,
        filePath,
        lastPosition,
        watched,
      );
      if (!episodes) continue;
      if (episodes === season.episodes) return library;
      const seasons = show.seasons.slice();
      seasons[k] = { ...season, episodes };
      const series = library.series.slice();
      series[s] = { ...show, seasons };
      return { ...library, series };
    }
  }

  for (const key of ["movies", "others"]) {
    const items = patchItems(library[key], filePath, lastPosition, watched);
    if (!items) continue;
    if (items === library[key]) return library;
    return { ...library, [key]: items };
  }

  return library;
}

export function updateEpisodeProgress(filePath, lastPosition, watched) {
  if (!cachedLibrary) return;

  const newLibrary = patchLibrary(
    cachedLibrary,
    filePath,
    lastPosition,
    watched,
  );
  if (newLibrary === cachedLibrary) return;

  cachedLibrary = newLibrary;
  listeners.forEach((l) => l(newLibrary));
}

export function subscribe(listener) {
  listeners.push(listener);
  return () => {
    listeners = listeners.filter((l) => l !== listener);
  };
}

export function getPlayingFilePath() {
  return playingFilePath;
}

export function setPlayingFilePath(filePath) {
  playingFilePath = filePath;
  playingListeners.forEach((l) => l(filePath));
}

export function subscribePlaying(listener) {
  playingListeners.push(listener);
  return () => {
    playingListeners = playingListeners.filter((l) => l !== listener);
  };
}
