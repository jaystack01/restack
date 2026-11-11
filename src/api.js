// Thin wrappers around the Tauri commands registered in src-tauri/src/lib.rs.
// Every backend call goes through here, so command names and argument shapes
// live in one place. Tauri maps the camelCase arg keys to snake_case params.
// Each wrapper returns the invoke() promise unchanged; callers own the error
// handling.
import { invoke } from "@tauri-apps/api/core";
import { setLibrary } from "./store/libraryStore";

/**
 * @typedef {Object} LibraryResponse
 * @property {Array<{groupId: number, displayName: string, seasons: Array<{season: number, episodes: Object[]}>}>} series
 * @property {Object[]} movies
 * @property {Object[]} others
 */

// ── Scanning ──────────────────────────────────────────────────────────────

/**
 * Scan every default root ($HOME, /media/<user>, /mnt/*) and sync the DB.
 * Rejects with "A scan is already running" if another scan holds the guard.
 * @returns {Promise<LibraryResponse>}
 */
export function scanUserStorage() {
  return invoke("scan_user_storage");
}

/**
 * Scan a single root and sync only the rows under it.
 * @param {string} rootPath
 * @returns {Promise<LibraryResponse>}
 */
export function scanAndSync(rootPath) {
  return invoke("scan_and_sync", { rootPath });
}

/**
 * Default scan roots discovered on this machine.
 * @returns {Promise<string[]>}
 */
export function getScanRoots() {
  return invoke("get_scan_roots");
}

// ── Library ───────────────────────────────────────────────────────────────

/**
 * Library as stored in the DB, without scanning.
 * @returns {Promise<LibraryResponse>}
 */
export function getLibraryGrouped() {
  return invoke("get_library_grouped");
}

/**
 * Fetch the library from the DB and publish it to the store.
 * @returns {Promise<LibraryResponse>} the data passed to setLibrary
 */
export async function refreshLibrary() {
  const data = await getLibraryGrouped();
  setLibrary(data);
  return data;
}

/**
 * The single Continue Watching / Up Next item for the dashboard.
 * @returns {Promise<Object|null>} ContinueWatchingItem (`mode`, `upNextType`,
 *   `displayName`, `filePath`, `season`, `episode`, `lastPosition`,
 *   `watched`, `duration`, `thumbnailPath`, …) or null
 */
export function getContinueWatching() {
  return invoke("get_continue_watching_single_cmd");
}

/**
 * Move live media files to the Trash and soft-delete their rows. The backend
 * resolves paths from the ids and rejects the whole batch, before trashing
 * anything, if any id is unknown or not live. Rejects while a scan runs.
 * @param {number[]} mediaIds
 * @returns {Promise<void>}
 */
export function deleteEpisodes(mediaIds) {
  return invoke("delete_episodes_cmd", { mediaIds });
}

/**
 * Restore soft-deleted media files from the Trash and clear their soft
 * delete. Rejects the whole batch if any id is unknown or not trashed, and
 * while a scan runs.
 * @param {number[]} mediaIds
 * @returns {Promise<void>}
 */
export function restoreEpisodes(mediaIds) {
  return invoke("restore_episodes_cmd", { mediaIds });
}

/**
 * Reclassify media and pin it so rescans keep the choice.
 * @param {number[]} mediaIds
 * @param {'series'|'movie'|'other'} targetType
 * @param {number|null} targetGroupId series group, or null for movie/other
 * @returns {Promise<void>}
 */
export function reclassify(mediaIds, targetType, targetGroupId) {
  return invoke("reclassify_media_cmd", {
    mediaIds,
    targetType,
    targetGroupId,
  });
}

// ── Playback ──────────────────────────────────────────────────────────────

/**
 * Play a file in mpv. With startSeconds null the backend resumes from the
 * saved position (or 0 if it was in the credits).
 * @param {string} filePath
 * @param {number|null} [startSeconds=null]
 * @returns {Promise<void>}
 */
export function playMedia(filePath, startSeconds = null) {
  return invoke("play_episode", { filePath, startSeconds });
}

/**
 * Mark media watched/unwatched (position resets to 0).
 * @param {number[]} mediaIds
 * @param {boolean} watched
 * @returns {Promise<void>}
 */
export function setWatched(mediaIds, watched) {
  return invoke("set_watched_cmd", { mediaIds, watched });
}

// ── Groups ────────────────────────────────────────────────────────────────

/**
 * Change a group's display name (its normalized key stays the same).
 * @param {number} groupId
 * @param {string} newName
 * @returns {Promise<void>}
 */
export function renameGroup(groupId, newName) {
  return invoke("rename_group_cmd", { groupId, newName });
}

/**
 * Move every episode of the source group into the target group.
 * @param {number} sourceGroupId
 * @param {number} targetGroupId
 * @returns {Promise<void>}
 */
export function mergeGroups(sourceGroupId, targetGroupId) {
  return invoke("merge_groups_cmd", { sourceGroupId, targetGroupId });
}

/**
 * Move episodes into another series group (pins them).
 * @param {number[]} mediaIds
 * @param {number} targetGroupId
 * @returns {Promise<void>}
 */
export function moveEpisodes(mediaIds, targetGroupId) {
  return invoke("move_episodes_cmd", { mediaIds, targetGroupId });
}

/**
 * All series groups with their episode counts.
 * @returns {Promise<Array<{groupId: number, displayName: string, episodeCount: number}>>}
 */
export function getAllGroups() {
  return invoke("get_all_groups_cmd");
}

/**
 * Pairs of groups with near-identical names.
 * @returns {Promise<Array<{groupIdA: number, displayNameA: string, episodeCountA: number, groupIdB: number, displayNameB: string, episodeCountB: number}>>}
 */
export function getMergeSuggestions() {
  return invoke("get_merge_suggestions_cmd");
}

// ── System ────────────────────────────────────────────────────────────────

/**
 * Whether the runtime tools are on PATH. The backend only looks for the
 * executables; it doesn't run them.
 * @returns {Promise<{mpv: boolean, ffmpeg: boolean, ffprobe: boolean}>}
 */
export function checkDependencies() {
  return invoke("check_dependencies");
}
