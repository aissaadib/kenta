//! The editor's timeline model.
//!
//! This module knows nothing about FFmpeg, Tauri or the file system: it is the
//! set of rules that decides what a valid edit is. Keeping it pure means the
//! rules can be tested directly instead of through a UI or a render.
//!
//! Model:
//! * a [`Timeline`] holds one [`Track`] per media kind;
//! * a [`Track`] is an ordered list of non-overlapping [`Segment`]s;
//! * a [`Segment`] maps a range of the source media (`source_in..source_out`)
//!   onto a position on the timeline (`start`).
//!
//! Gaps are not stored. They are the space between segments, and are reported
//! by [`Timeline::gaps`] so the exporter can fill them with black video or
//! silence. Moving a segment never shifts anything on another track.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Shortest segment the model accepts. Two frames at 60 fps is ~0.033 s, so
/// 0.02 s keeps every accepted segment representable while still rejecting the
/// zero-length results of splitting exactly on a boundary.
pub const MIN_SEGMENT_SECONDS: f64 = 0.02;

const EPSILON: f64 = 1e-6;
const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
  Video,
  Pc,
  Mic,
  Mixed,
}

impl TrackKind {
  pub fn is_audio(self) -> bool {
    !matches!(self, TrackKind::Video)
  }

  pub fn label(self) -> &'static str {
    match self {
      TrackKind::Video => "Video",
      TrackKind::Pc => "PC Audio",
      TrackKind::Mic => "Microphone",
      TrackKind::Mixed => "Mixed Audio",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
  /// Stable for the lifetime of the edit; a split mints a new id for the new
  /// right-hand part so the two halves are never confused.
  pub id: u64,
  pub track: TrackKind,
  pub source_in: f64,
  pub source_out: f64,
  /// Position of `source_in` on the timeline.
  pub start: f64,
}

impl Segment {
  pub fn duration(&self) -> f64 {
    self.source_out - self.source_in
  }

  pub fn end(&self) -> f64 {
    self.start + self.duration()
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
  pub kind: TrackKind,
  pub segments: Vec<Segment>,
}

impl Track {
  fn sorted(mut self) -> Self {
    self.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
    self
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEdge {
  In,
  Out,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
  NotFound(u64),
  InvalidRange(String),
  Overlap(String),
  NoAudioTrack(TrackKind),
}

impl fmt::Display for EditError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      EditError::NotFound(id) => write!(f, "That segment no longer exists ({id})."),
      EditError::InvalidRange(message) => write!(f, "{message}"),
      EditError::Overlap(message) => write!(f, "{message}"),
      EditError::NoAudioTrack(kind) => {
        write!(f, "This clip has no {} track.", kind.label().to_lowercase())
      }
    }
  }
}

impl std::error::Error for EditError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineView {
  pub duration: f64,
  pub media_duration: f64,
  pub segments: Vec<Segment>,
  pub can_undo: bool,
  pub can_redo: bool,
}

/// The whole editable state. Cloning it is cheap (a handful of segments) and
/// that is what makes undo exact.
#[derive(Debug, Clone)]
struct State {
  tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct Timeline {
  media_duration: f64,
  /// Length of the rendered clip. Fixed for the session so deleting or moving
  /// a segment leaves an explicit gap instead of silently shortening the clip.
  duration: f64,
  tracks: Vec<Track>,
  next_id: u64,
  undo_stack: Vec<State>,
  redo_stack: Vec<State>,
}

impl Timeline {
  /// Start from the untouched clip: every track is one segment covering the
  /// whole media, so source time and timeline time agree until the user edits.
  pub fn new(media_duration: f64, tracks: &[TrackKind]) -> Self {
    let mut next_id = 1;
    let tracks: Vec<Track> = tracks
      .iter()
      .map(|kind| {
        let segment = Segment {
          id: next_id,
          track: *kind,
          source_in: 0.0,
          source_out: media_duration,
          start: 0.0,
        };
        next_id += 1;
        Track {
          kind: *kind,
          segments: vec![segment],
        }
      })
      .collect();
    Self {
      media_duration,
      duration: media_duration,
      tracks,
      next_id,
      undo_stack: Vec::new(),
      redo_stack: Vec::new(),
    }
  }

  pub fn media_duration(&self) -> f64 {
    self.media_duration
  }

  pub fn duration(&self) -> f64 {
    self.duration
  }

  pub fn tracks(&self) -> &[Track] {
    &self.tracks
  }

  pub fn track(&self, kind: TrackKind) -> Option<&Track> {
    self.tracks.iter().find(|track| track.kind == kind)
  }

  pub fn segments(&self) -> Vec<Segment> {
    let mut all: Vec<Segment> = self
      .tracks
      .iter()
      .flat_map(|track| track.segments.iter().cloned())
      .collect();
    all.sort_by(|a, b| {
      a.start
        .total_cmp(&b.start)
        .then_with(|| a.track.label().cmp(b.track.label()))
    });
    all
  }

  pub fn view(&self) -> TimelineView {
    TimelineView {
      duration: self.duration,
      media_duration: self.media_duration,
      segments: self.segments(),
      can_undo: !self.undo_stack.is_empty(),
      can_redo: !self.redo_stack.is_empty(),
    }
  }

  /// Spaces on a track with no segment, in timeline order.
  pub fn gaps(&self, kind: TrackKind) -> Vec<(f64, f64)> {
    let Some(track) = self.track(kind) else {
      return Vec::new();
    };
    let mut gaps = Vec::new();
    let mut cursor = 0.0;
    for segment in &track.segments {
      if segment.start > cursor + EPSILON {
        gaps.push((cursor, segment.start));
      }
      cursor = cursor.max(segment.end());
    }
    if self.duration > cursor + EPSILON {
      gaps.push((cursor, self.duration));
    }
    gaps
  }

  /// Split the segment that covers `at` into two adjacent segments.
  pub fn split(&mut self, id: u64, at: f64) -> Result<(), EditError> {
    let (track_index, segment_index) = self.locate(id)?;
    let segment = self.tracks[track_index].segments[segment_index].clone();
    if !at.is_finite()
      || at <= segment.start + MIN_SEGMENT_SECONDS
      || at >= segment.end() - MIN_SEGMENT_SECONDS
    {
      return Err(EditError::InvalidRange(format!(
        "The playhead is too close to the edge of the segment to split it. Move it inside the segment (at least {MIN_SEGMENT_SECONDS:.2}s from either end)."
      )));
    }
    let snapshot = self.snapshot();
    let offset = at - segment.start;
    let left = Segment {
      source_out: segment.source_in + offset,
      ..segment.clone()
    };
    let right = Segment {
      id: self.mint_id(),
      start: at,
      source_in: segment.source_in + offset,
      ..segment
    };
    self.tracks[track_index].segments[segment_index] = left;
    self.tracks[track_index]
      .segments
      .insert(segment_index + 1, right);
    self.commit(snapshot);
    Ok(())
  }

  /// Delete one segment. Only its own track changes.
  pub fn delete(&mut self, id: u64) -> Result<(), EditError> {
    let (track_index, segment_index) = self.locate(id)?;
    let snapshot = self.snapshot();
    self.tracks[track_index].segments.remove(segment_index);
    self.commit(snapshot);
    Ok(())
  }

  /// Drag one edge of a segment to a timeline position.
  ///
  /// Trimming the in point moves the segment's start with it, so the visible
  /// content does not jump; trimming the out point leaves the start alone.
  pub fn trim(&mut self, id: u64, edge: TrimEdge, to: f64) -> Result<(), EditError> {
    let (track_index, segment_index) = self.locate(id)?;
    let segment = self.tracks[track_index].segments[segment_index].clone();
    if !to.is_finite() {
      return Err(EditError::InvalidRange("The trim position is not a number.".into()));
    }
    let snapshot = self.snapshot();
    match edge {
      TrimEdge::In => {
        let new_source_in = segment.source_in + (to - segment.start);
        if to < -EPSILON {
          return Err(EditError::InvalidRange(
            "A segment cannot start before the beginning of the clip.".into(),
          ));
        }
        if new_source_in < -EPSILON {
          return Err(EditError::InvalidRange(
            "That would trim past the start of the source media.".into(),
          ));
        }
        if to >= segment.end() - MIN_SEGMENT_SECONDS {
          return Err(EditError::InvalidRange(
            "A segment must keep a positive duration.".into(),
          ));
        }
        if let Some(previous) = segment_index.checked_sub(1).map(|i| &self.tracks[track_index].segments[i]) {
          if to < previous.end() - EPSILON {
            return Err(EditError::Overlap(format!(
              "That would overlap the previous segment on the {} track.",
              segment.track.label()
            )));
          }
        }
        let target = &mut self.tracks[track_index].segments[segment_index];
        target.start = to.max(0.0);
        target.source_in = new_source_in.max(0.0);
      }
      TrimEdge::Out => {
        let new_source_out = segment.source_in + (to - segment.start);
        if to <= segment.start + MIN_SEGMENT_SECONDS {
          return Err(EditError::InvalidRange(
            "A segment must keep a positive duration.".into(),
          ));
        }
        if to > self.duration + EPSILON {
          return Err(EditError::InvalidRange(
            "That would extend the segment past the end of the clip.".into(),
          ));
        }
        if new_source_out > self.media_duration + EPSILON {
          return Err(EditError::InvalidRange(
            "There is no more source media to extend this segment with.".into(),
          ));
        }
        if let Some(next) = self.tracks[track_index].segments.get(segment_index + 1) {
          if to > next.start + EPSILON {
            return Err(EditError::Overlap(format!(
              "That would overlap the next segment on the {} track.",
              segment.track.label()
            )));
          }
        }
        self.tracks[track_index].segments[segment_index].source_out = new_source_out;
      }
    }
    self.commit(snapshot);
    Ok(())
  }

  /// Move a segment horizontally on its own track. The source range is
  /// untouched, so the content stays in sync with whatever the user sees.
  pub fn move_to(&mut self, id: u64, start: f64) -> Result<(), EditError> {
    let (track_index, segment_index) = self.locate(id)?;
    let segment = self.tracks[track_index].segments[segment_index].clone();
    if !start.is_finite() {
      return Err(EditError::InvalidRange("The position is not a number.".into()));
    }
    if start < -EPSILON {
      return Err(EditError::InvalidRange(
        "A segment cannot start before the beginning of the clip.".into(),
      ));
    }
    let end = start + segment.duration();
    if end > self.duration + EPSILON {
      return Err(EditError::InvalidRange(
        "That would push the segment past the end of the clip.".into(),
      ));
    }
    for other in &self.tracks[track_index].segments {
      if other.id == id {
        continue;
      }
      if start < other.end() - EPSILON && end > other.start + EPSILON {
        return Err(EditError::Overlap(format!(
          "That would overlap another segment on the {} track.",
          segment.track.label()
        )));
      }
    }
    let snapshot = self.snapshot();
    self.tracks[track_index].segments[segment_index].start = start;
    self.tracks[track_index] = self.tracks[track_index].clone().sorted();
    self.commit(snapshot);
    Ok(())
  }

  pub fn undo(&mut self) -> bool {
    let Some(previous) = self.undo_stack.pop() else {
      return false;
    };
    self.redo_stack.push(State {
      tracks: self.tracks.clone(),
    });
    self.tracks = previous.tracks;
    true
  }

  pub fn redo(&mut self) -> bool {
    let Some(next) = self.redo_stack.pop() else {
      return false;
    };
    self.undo_stack.push(State {
      tracks: self.tracks.clone(),
    });
    self.tracks = next.tracks;
    true
  }

  pub fn can_undo(&self) -> bool {
    !self.undo_stack.is_empty()
  }

  pub fn can_redo(&self) -> bool {
    !self.redo_stack.is_empty()
  }

  /// Re-check every invariant. Used after loading a timeline and in tests.
  pub fn validate(&self) -> Result<(), EditError> {
    if !self.media_duration.is_finite() || self.media_duration <= 0.0 {
      return Err(EditError::InvalidRange(
        "The clip has no usable duration.".into(),
      ));
    }
    for track in &self.tracks {
      let mut previous_end = 0.0;
      for segment in &track.segments {
        if segment.duration() < MIN_SEGMENT_SECONDS - EPSILON {
          return Err(EditError::InvalidRange(format!(
            "A {} segment is too short.",
            track.kind.label()
          )));
        }
        if segment.source_in < -EPSILON || segment.source_out > self.media_duration + EPSILON {
          return Err(EditError::InvalidRange(format!(
            "A {} segment points outside the source media.",
            track.kind.label()
          )));
        }
        if segment.start < -EPSILON || segment.end() > self.duration + EPSILON {
          return Err(EditError::InvalidRange(format!(
            "A {} segment sits outside the timeline.",
            track.kind.label()
          )));
        }
        if segment.start < previous_end - EPSILON {
          return Err(EditError::Overlap(format!(
            "Two {} segments overlap.",
            track.kind.label()
          )));
        }
        previous_end = segment.end();
      }
    }
    Ok(())
  }

  fn locate(&self, id: u64) -> Result<(usize, usize), EditError> {
    for (track_index, track) in self.tracks.iter().enumerate() {
      if let Some(segment_index) = track.segments.iter().position(|s| s.id == id) {
        return Ok((track_index, segment_index));
      }
    }
    Err(EditError::NotFound(id))
  }

  fn mint_id(&mut self) -> u64 {
    let id = self.next_id;
    self.next_id += 1;
    id
  }

  fn snapshot(&self) -> State {
    State {
      tracks: self.tracks.clone(),
    }
  }

  fn commit(&mut self, snapshot: State) {
    self.undo_stack.push(snapshot);
    if self.undo_stack.len() > HISTORY_LIMIT {
      self.undo_stack.remove(0);
    }
    self.redo_stack.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn timeline() -> Timeline {
    Timeline::new(
      10.0,
      &[TrackKind::Video, TrackKind::Pc, TrackKind::Mic],
    )
  }

  fn only(timeline: &Timeline, kind: TrackKind) -> Vec<Segment> {
    timeline.track(kind).unwrap().segments.clone()
  }

  #[test]
  fn starts_with_one_full_length_segment_per_track() {
    let timeline = timeline();
    assert_eq!(timeline.duration(), 10.0);
    for kind in [TrackKind::Video, TrackKind::Pc, TrackKind::Mic] {
      let segments = only(&timeline, kind);
      assert_eq!(segments.len(), 1);
      assert_eq!(segments[0].source_in, 0.0);
      assert_eq!(segments[0].source_out, 10.0);
      assert_eq!(segments[0].start, 0.0);
    }
    assert!(timeline.gaps(TrackKind::Video).is_empty());
  }

  #[test]
  fn split_in_the_middle_creates_two_adjacent_segments() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(id, 4.0).unwrap();
    let segments = only(&timeline, TrackKind::Video);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].source_in, 0.0);
    assert_eq!(segments[0].source_out, 4.0);
    assert_eq!(segments[1].source_in, 4.0);
    assert_eq!(segments[1].source_out, 10.0);
    assert_eq!(segments[1].start, 4.0);
    assert_ne!(segments[0].id, segments[1].id);
    assert!(timeline.gaps(TrackKind::Video).is_empty());
    assert!(timeline.validate().is_ok());
  }

  #[test]
  fn split_at_the_edges_is_rejected() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    assert!(timeline.split(id, 0.0).is_err());
    assert!(timeline.split(id, 10.0).is_err());
    assert!(timeline.split(id, 10.5).is_err());
    assert_eq!(only(&timeline, TrackKind::Video).len(), 1);
    assert!(!timeline.can_undo());
  }

  #[test]
  fn split_only_touches_its_own_track() {
    let mut timeline = timeline();
    let video = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(video, 3.0).unwrap();
    assert_eq!(only(&timeline, TrackKind::Video).len(), 2);
    assert_eq!(only(&timeline, TrackKind::Pc).len(), 1);
    assert_eq!(only(&timeline, TrackKind::Mic).len(), 1);
  }

  #[test]
  fn delete_removes_only_its_own_segment() {
    let mut timeline = timeline();
    let video = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(video, 5.0).unwrap();
    let first = only(&timeline, TrackKind::Video)[0].id;
    timeline.delete(first).unwrap();
    let segments = only(&timeline, TrackKind::Video);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].start, 5.0);
    assert_eq!(segments[0].source_in, 5.0);
    assert_eq!(only(&timeline, TrackKind::Pc).len(), 1);
    assert_eq!(only(&timeline, TrackKind::Mic).len(), 1);
    // The hole is explicit, not closed by shifting the remaining segment.
    assert_eq!(timeline.gaps(TrackKind::Video), vec![(0.0, 5.0)]);
  }

  #[test]
  fn deleting_everything_leaves_one_full_length_gap() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.delete(id).unwrap();
    assert_eq!(timeline.gaps(TrackKind::Video), vec![(0.0, 10.0)]);
    assert!(timeline.validate().is_ok());
  }

  #[test]
  fn moving_a_segment_keeps_its_source_range_and_other_tracks() {
    let mut timeline = timeline();
    let video = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(video, 4.0).unwrap();
    let first = only(&timeline, TrackKind::Video)[0].id;
    let second = only(&timeline, TrackKind::Video)[1].id;
    timeline.move_to(second, 0.0).unwrap_err(); // would overlap the first half
    timeline.trim(first, TrimEdge::Out, 2.0).unwrap();
    timeline.move_to(second, 2.0).unwrap();
    let segments = only(&timeline, TrackKind::Video);
    assert_eq!(segments[1].start, 2.0);
    assert_eq!(segments[1].source_in, 4.0);
    assert_eq!(segments[1].source_out, 10.0);
    assert_eq!(only(&timeline, TrackKind::Pc)[0].start, 0.0);
    assert_eq!(only(&timeline, TrackKind::Mic)[0].start, 0.0);
    assert_eq!(timeline.gaps(TrackKind::Video), vec![(8.0, 10.0)]);
  }

  #[test]
  fn moving_past_the_end_of_the_clip_is_rejected() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.trim(id, TrimEdge::Out, 4.0).unwrap();
    assert!(timeline.move_to(id, 6.5).is_err());
    assert!(timeline.move_to(id, -1.0).is_err());
    assert_eq!(only(&timeline, TrackKind::Video)[0].start, 0.0);
  }

  #[test]
  fn trimming_the_in_point_moves_the_start_with_the_content() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.trim(id, TrimEdge::In, 2.0).unwrap();
    let segment = &only(&timeline, TrackKind::Video)[0];
    assert_eq!(segment.start, 2.0);
    assert_eq!(segment.source_in, 2.0);
    assert_eq!(segment.source_out, 10.0);
    assert_eq!(segment.duration(), 8.0);
    assert_eq!(timeline.gaps(TrackKind::Video), vec![(0.0, 2.0)]);
  }

  #[test]
  fn trimming_the_out_point_stops_at_the_end_of_the_media() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.trim(id, TrimEdge::Out, 4.0).unwrap();
    assert_eq!(only(&timeline, TrackKind::Video)[0].duration(), 4.0);
    // Extending again is allowed while source media remains.
    timeline.trim(id, TrimEdge::Out, 7.0).unwrap();
    assert_eq!(only(&timeline, TrackKind::Video)[0].source_out, 7.0);
    assert!(timeline.trim(id, TrimEdge::Out, 12.0).is_err());
  }

  #[test]
  fn trimming_to_zero_or_negative_duration_is_rejected() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.trim(id, TrimEdge::Out, 4.0).unwrap();
    assert!(timeline.trim(id, TrimEdge::Out, 0.0).is_err());
    assert!(timeline.trim(id, TrimEdge::Out, 0.01).is_err());
    assert!(timeline.trim(id, TrimEdge::In, 4.0).is_err());
    assert!(timeline.trim(id, TrimEdge::In, 5.0).is_err());
    assert!(timeline.trim(id, TrimEdge::Out, f64::NAN).is_err());
  }

  #[test]
  fn trimming_cannot_overlap_a_neighbour() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(id, 5.0).unwrap();
    let first = only(&timeline, TrackKind::Video)[0].id;
    assert!(timeline.trim(first, TrimEdge::Out, 6.0).is_err());
    let second = only(&timeline, TrackKind::Video)[1].id;
    assert!(timeline.trim(second, TrimEdge::In, 4.0).is_err());
  }

  #[test]
  fn unknown_segment_ids_are_rejected() {
    let mut timeline = timeline();
    assert!(timeline.split(999, 5.0).is_err());
    assert!(timeline.delete(999).is_err());
    assert!(timeline.trim(999, TrimEdge::In, 1.0).is_err());
    assert!(timeline.move_to(999, 1.0).is_err());
    assert!(!timeline.can_undo());
  }

  #[test]
  fn undo_and_redo_restore_the_exact_state() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(id, 4.0).unwrap();
    let after_split = timeline.segments();
    timeline.delete(only(&timeline, TrackKind::Video)[1].id).unwrap();
    assert_eq!(only(&timeline, TrackKind::Video).len(), 1);

    assert!(timeline.undo());
    assert_eq!(timeline.segments(), after_split);
    assert!(timeline.undo());
    assert_eq!(only(&timeline, TrackKind::Video).len(), 1);
    assert!(!timeline.undo());

    assert!(timeline.redo());
    assert_eq!(only(&timeline, TrackKind::Video).len(), 2);
    assert!(timeline.redo());
    assert_eq!(only(&timeline, TrackKind::Video).len(), 1);
    assert!(!timeline.redo());
  }

  #[test]
  fn a_new_edit_clears_the_redo_stack() {
    let mut timeline = timeline();
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(id, 4.0).unwrap();
    assert!(timeline.undo());
    assert!(timeline.can_redo());
    timeline.split(only(&timeline, TrackKind::Video)[0].id, 2.0).unwrap();
    assert!(!timeline.can_redo());
  }

  #[test]
  fn undo_after_every_operation_returns_to_the_original() {
    let mut timeline = timeline();
    let original = timeline.segments();
    let video = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(video, 3.0).unwrap();
    timeline
      .trim(only(&timeline, TrackKind::Video)[1].id, TrimEdge::Out, 7.0)
      .unwrap();
    timeline
      .move_to(only(&timeline, TrackKind::Video)[1].id, 5.0)
      .unwrap();
    timeline.delete(only(&timeline, TrackKind::Pc)[0].id).unwrap();
    timeline
      .trim(only(&timeline, TrackKind::Mic)[0].id, TrimEdge::In, 1.0)
      .unwrap();
    let mut steps = 0;
    while timeline.undo() {
      steps += 1;
    }
    assert_eq!(steps, 5);
    assert_eq!(timeline.segments(), original);
  }

  #[test]
  fn gap_after_a_trimmed_in_point_is_reported_for_that_track_only() {
    let mut timeline = timeline();
    let mic = only(&timeline, TrackKind::Mic)[0].id;
    timeline.trim(mic, TrimEdge::In, 3.0).unwrap();
    assert_eq!(timeline.gaps(TrackKind::Mic), vec![(0.0, 3.0)]);
    assert!(timeline.gaps(TrackKind::Pc).is_empty());
    assert!(timeline.gaps(TrackKind::Video).is_empty());
  }

  #[test]
  fn audio_offset_stays_independent_of_video() {
    let mut timeline = timeline();
    let pc = only(&timeline, TrackKind::Pc)[0].id;
    timeline.trim(pc, TrimEdge::In, 1.5).unwrap();
    timeline.trim(pc, TrimEdge::Out, 8.0).unwrap();
    let video = &only(&timeline, TrackKind::Video)[0];
    assert_eq!(video.start, 0.0);
    assert_eq!(video.source_out, 10.0);
    let pc = &only(&timeline, TrackKind::Pc)[0];
    assert_eq!(pc.start, 1.5);
    assert_eq!(pc.source_in, 1.5);
    assert_eq!(pc.source_out, 8.0);
  }

  #[test]
  fn a_video_only_clip_has_no_audio_tracks() {
    let mut timeline = Timeline::new(6.0, &[TrackKind::Video]);
    assert!(timeline.track(TrackKind::Pc).is_none());
    assert!(timeline.gaps(TrackKind::Pc).is_empty());
    assert!(timeline.track(TrackKind::Video).is_some());
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(id, 2.0).unwrap();
    assert!(timeline.validate().is_ok());
  }

  #[test]
  fn a_legacy_clip_gets_one_mixed_audio_track() {
    let timeline = Timeline::new(6.0, &[TrackKind::Video, TrackKind::Mixed]);
    assert!(timeline.track(TrackKind::Mixed).is_some());
    assert!(timeline.track(TrackKind::Pc).is_none());
    assert!(timeline.track(TrackKind::Mic).is_none());
    assert_eq!(only(&timeline, TrackKind::Mixed).len(), 1);
  }

  #[test]
  fn view_reports_history_availability() {
    let mut timeline = timeline();
    assert!(!timeline.view().can_undo);
    let id = only(&timeline, TrackKind::Video)[0].id;
    timeline.split(id, 5.0).unwrap();
    assert!(timeline.view().can_undo);
    timeline.undo();
    assert!(timeline.view().can_redo);
  }
}
