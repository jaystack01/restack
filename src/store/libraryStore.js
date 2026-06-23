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

export function updateEpisodeProgress(filePath, lastPosition, watched) {
  if (!cachedLibrary) return;

  const newLibrary = {
    series: cachedLibrary.series.map((show) => ({
      ...show,
      seasons: show.seasons.map((season) => ({
        ...season,
        episodes: season.episodes.map((ep) => {
          if (ep.filePath === filePath) {
            return { ...ep, lastPosition, watched };
          }
          return ep;
        }),
      })),
    })),
    movies: cachedLibrary.movies.map((m) => {
      if (m.filePath === filePath) {
        return { ...m, lastPosition, watched };
      }
      return m;
    }),
    others: cachedLibrary.others.map((o) => {
      if (o.filePath === filePath) {
        return { ...o, lastPosition, watched };
      }
      return o;
    }),
  };

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
