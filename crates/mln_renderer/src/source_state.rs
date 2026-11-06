// Ports src/mln/renderer/source_state.hpp and src/mln/renderer/source_state.cpp
//
// `coalesceChanges(std::vector<RenderTile>&)` calls `tile.setFeatureState`
// on every render tile; a Rust RenderTile has no reference to its Tile, so
// `coalesce_changes` returns the states and the render source applies them
// to the tiles of its pyramid (the same tiles).

use mln_style::expression::expression::FeatureState;

use crate::bucket::{FeatureStates, LayerFeatureStates};

/// `SourceFeatureState`.
#[derive(Debug, Default)]
pub struct SourceFeatureState {
    current_states: LayerFeatureStates,
    state_changes: LayerFeatureStates,
    deleted_states: LayerFeatureStates,
}

impl SourceFeatureState {
    pub fn new() -> Self {
        Self::default()
    }

    /// `updateState(sourceLayerID, featureID, newState)`.
    pub fn update_state(&mut self, source_layer_id: Option<&str>, feature_id: &str, new_state: &FeatureState) {
        let source_layer = source_layer_id.unwrap_or("").to_owned();
        for (key, value) in new_state {
            let layer_states = self.state_changes.entry(source_layer.clone()).or_default();
            let feature_states = layer_states.entry(feature_id.to_owned()).or_default();
            feature_states.insert(key.clone(), value.clone());
        }
    }

    /// `getState(result, sourceLayerID, featureID)`.
    pub fn get_state(&self, source_layer_id: Option<&str>, feature_id: &str) -> FeatureState {
        let source_layer = source_layer_id.unwrap_or("");
        let current = self
            .current_states
            .get(source_layer)
            .and_then(|layer_states| layer_states.get(feature_id))
            .cloned()
            .unwrap_or_default();
        let changes = self
            .state_changes
            .get(source_layer)
            .and_then(|layer_states| layer_states.get(feature_id))
            .cloned()
            .unwrap_or_default();
        // result = changes; result.insert(current.begin(), current.end()):
        // insert keeps the existing (changed) values.
        let mut result = changes;
        for (key, value) in current {
            result.entry(key).or_insert(value);
        }
        result
    }

    /// `coalesceChanges(tiles)`: folds the queued changes and deletions into
    /// the current states and returns what C++ passes to every tile's
    /// `setFeatureState`, or `None` when it returns early (nothing to
    /// apply).
    pub fn coalesce_changes(&mut self) -> Option<LayerFeatureStates> {
        let mut changes: LayerFeatureStates = LayerFeatureStates::new();
        let state_changes = std::mem::take(&mut self.state_changes);
        for (source_layer, layer_state_changes) in &state_changes {
            let mut layer_states = FeatureStates::new();
            for (feature_id, feature_state_changes) in layer_state_changes {
                let current =
                    self.current_states.entry(source_layer.clone()).or_default().entry(feature_id.clone()).or_default();
                for (state_key, state_val) in feature_state_changes {
                    current.insert(state_key.clone(), state_val.clone());
                }
                layer_states.insert(feature_id.clone(), current.clone());
            }
            changes.insert(source_layer.clone(), layer_states);
        }

        let deleted_states = std::mem::take(&mut self.deleted_states);
        for (source_layer, layer_deleted_states) in &deleted_states {
            // `FeatureStates layerStates = {{}, {}};`: one entry with an
            // empty id and an empty state.
            let mut layer_states = FeatureStates::new();
            layer_states.insert(String::new(), FeatureState::new());

            let current_layer = self.current_states.entry(source_layer.clone()).or_default();
            if layer_deleted_states.is_empty() {
                let feature_ids: Vec<String> = current_layer.keys().cloned().collect();
                for feature_id in feature_ids {
                    layer_states.insert(feature_id.clone(), FeatureState::new());
                    current_layer.insert(feature_id, FeatureState::new());
                }
            } else {
                for (feature_id, deleted_keys) in layer_deleted_states {
                    let delete_whole_feature_state = deleted_keys.is_empty();
                    let current = current_layer.entry(feature_id.clone()).or_default();
                    if delete_whole_feature_state {
                        *current = FeatureState::new();
                    } else {
                        for state_key in deleted_keys.keys() {
                            current.remove(state_key);
                        }
                    }
                    layer_states.insert(feature_id.clone(), current.clone());
                }
            }
            changes.insert(source_layer.clone(), layer_states);
        }

        // changes.insert(currentStates.begin(), currentStates.end()): keeps
        // the entries already in `changes`.
        for (source_layer, layer_states) in &self.current_states {
            changes.entry(source_layer.clone()).or_insert_with(|| layer_states.clone());
        }

        if changes.is_empty() {
            return None;
        }
        Some(changes)
    }

    /// `removeState(sourceLayerID, featureID, stateKey)`.
    pub fn remove_state(&mut self, source_layer_id: Option<&str>, feature_id: Option<&str>, state_key: Option<&str>) {
        let source_layer = source_layer_id.unwrap_or("").to_owned();

        let source_layer_deleted =
            self.deleted_states.get(&source_layer).is_some_and(|layer_states| layer_states.is_empty());
        if source_layer_deleted {
            return;
        }

        match (state_key, feature_id) {
            (Some(state_key), Some(feature_id)) => {
                // `!deletedStates.contains(sourceLayer) &&
                // !deletedStates[sourceLayer].contains(featureID)`: the
                // second operand's operator[] inserts the layer; when the
                // layer is already there, nothing is recorded (C++ quirk).
                if !self.deleted_states.contains_key(&source_layer) {
                    let layer_states = self.deleted_states.entry(source_layer).or_default();
                    if !layer_states.contains_key(feature_id) {
                        layer_states
                            .entry(feature_id.to_owned())
                            .or_default()
                            .insert(state_key.to_owned(), Default::default());
                    }
                }
            }
            (None, Some(feature_id)) => {
                let update_in_queue = self
                    .state_changes
                    .get(&source_layer)
                    .is_some_and(|layer_states| layer_states.contains_key(feature_id));
                if update_in_queue {
                    let keys: Vec<String> = self.state_changes[&source_layer][feature_id].keys().cloned().collect();
                    let deleted =
                        self.deleted_states.entry(source_layer).or_default().entry(feature_id.to_owned()).or_default();
                    for key in keys {
                        deleted.insert(key, Default::default());
                    }
                } else {
                    self.deleted_states
                        .entry(source_layer)
                        .or_default()
                        .insert(feature_id.to_owned(), FeatureState::new());
                }
            }
            _ => {
                self.deleted_states.insert(source_layer, FeatureStates::new());
            }
        }
    }
}
