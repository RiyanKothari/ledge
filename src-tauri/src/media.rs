//! Now playing from Windows' media sessions (Spotify, browsers, any app that
//! shows in the volume flyout). Polled once a second; artwork read once per track.

use crate::hub;
use base64::Engine;
use serde::Serialize;
use std::time::Duration;
use tauri::AppHandle;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Props,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;

const MAX_ART_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub source_app: String,
    pub playing: bool,
    pub art: Option<String>,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Unix ms when `position_ms` was true; the UI extrapolates from here.
    pub updated_at: i64,
    pub can_seek: bool,
    /// None when the app doesn't expose it.
    pub shuffle: Option<bool>,
    /// "none", "track" or "list"; None when the app doesn't expose it.
    pub repeat: Option<String>,
}

/// 100ns ticks between 1601-01-01 and 1970-01-01.
const EPOCH_DIFF_TICKS: i64 = 116_444_736_000_000_000;

fn manager() -> windows::core::Result<Manager> {
    Manager::RequestAsync()?.join()
}

/// A session that is playing wins; otherwise whatever Windows calls current.
fn pick(mgr: &Manager) -> Option<Session> {
    let sessions = mgr.GetSessions().ok()?;
    for s in &sessions {
        let playing = s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).ok() == Some(Status::Playing);
        if playing {
            return Some(s);
        }
    }
    mgr.GetCurrentSession().ok()
}

fn read_art(props: &Props) -> Option<String> {
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > MAX_ART_BYTES {
        return None;
    }
    let mime = stream.ContentType().map(|s| s.to_string()).unwrap_or_default();
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size as u32).ok()?.join().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let mime = if mime.is_empty() { "image/png".to_string() } else { mime };
    Some(format!("data:{};base64,{}", mime, base64::engine::general_purpose::STANDARD.encode(buf)))
}

pub fn control(action: &str, position_ms: Option<i64>) -> bool {
    let run = || -> windows::core::Result<bool> {
        let mgr = manager()?;
        let Some(s) = pick(&mgr) else { return Ok(false) };
        let op = match action {
            "toggle" => s.TryTogglePlayPauseAsync()?,
            "next" => s.TrySkipNextAsync()?,
            "previous" => s.TrySkipPreviousAsync()?,
            "seek" => s.TryChangePlaybackPositionAsync(position_ms.unwrap_or(0).max(0) * 10_000)?,
            "shuffle" => {
                let on = s.GetPlaybackInfo()?.IsShuffleActive().and_then(|v| v.Value()).unwrap_or(false);
                s.TryChangeShuffleActiveAsync(!on)?
            }
            "repeat" => {
                use windows::Media::MediaPlaybackAutoRepeatMode as R;
                let now = s.GetPlaybackInfo()?.AutoRepeatMode().and_then(|v| v.Value()).unwrap_or(R::None);
                let next = if now == R::None { R::List } else if now == R::List { R::Track } else { R::None };
                s.TryChangeAutoRepeatModeAsync(next)?
            }
            _ => return Ok(false),
        };
        op.join()
    };
    run().unwrap_or(false)
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-media".into())
        .spawn(move || {
            let mut mgr: Option<Manager> = None;
            let mut art_key = String::new();
            let mut art: Option<String> = None;
            loop {
                if mgr.is_none() {
                    mgr = manager().ok();
                }
                let now = mgr.as_ref().and_then(|m| {
                    let s = pick(m)?;
                    let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
                    let title = props.Title().ok()?.to_string();
                    if title.is_empty() {
                        return None;
                    }
                    let artist = props.Artist().map(|a| a.to_string()).unwrap_or_default();
                    let key = format!("{title}\u{1f}{artist}");
                    // Apps often set artwork a moment after the title: retry until it arrives.
                    if key != art_key || art.is_none() {
                        art = read_art(&props);
                        art_key = key;
                    }
                    let info = s.GetPlaybackInfo().ok();
                    let shuffle = info.as_ref().and_then(|i| i.IsShuffleActive().ok()).and_then(|v| v.Value().ok());
                    let repeat = info.as_ref().and_then(|i| i.AutoRepeatMode().ok()).and_then(|v| v.Value().ok()).map(|m| {
                        use windows::Media::MediaPlaybackAutoRepeatMode as R;
                        if m == R::Track { "track" } else if m == R::List { "list" } else { "none" }.to_string()
                    });
                    let playing = info.as_ref().and_then(|i| i.PlaybackStatus().ok()) == Some(Status::Playing);
                    let can_seek = info
                        .as_ref()
                        .and_then(|i| i.Controls().ok())
                        .and_then(|c| c.IsPlaybackPositionEnabled().ok())
                        .unwrap_or(false);
                    let tl = s.GetTimelineProperties().ok();
                    let pos = tl.as_ref().and_then(|t| t.Position().ok()).map(|d| d.Duration / 10_000).unwrap_or(0);
                    let dur = tl
                        .as_ref()
                        .and_then(|t| Some(t.EndTime().ok()?.Duration - t.StartTime().ok()?.Duration))
                        .map(|d| d / 10_000)
                        .unwrap_or(0);
                    let upd = tl
                        .as_ref()
                        .and_then(|t| t.LastUpdatedTime().ok())
                        .map(|t| (t.UniversalTime - EPOCH_DIFF_TICKS) / 10_000)
                        .unwrap_or(0);
                    Some(NowPlaying {
                        title,
                        artist,
                        source_app: s.SourceAppUserModelId().map(|a| a.to_string()).unwrap_or_default(),
                        playing,
                        art: art.clone(),
                        position_ms: pos,
                        duration_ms: dur.max(0),
                        updated_at: upd,
                        can_seek,
                        shuffle,
                        repeat,
                    })
                });
                hub::publish(&app, "media", &now);
                std::thread::sleep(Duration::from_millis(1000));
            }
        })
        .expect("spawn media watcher");
}
