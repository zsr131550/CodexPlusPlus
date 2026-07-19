use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Mutex, MutexGuard};

use codex_plus_core::settings::BackendSettings;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const DEFAULT_CAPACITY_PER_SCOPE: usize = 64;
const STEPWISE_FINGERPRINT_DOMAIN: &[u8] = b"codex-plus-manager/stepwise/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RevisionScope {
    AppPath,
    Enhancements,
    Stepwise,
    ImageOverlay,
    ExtraArgs,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RevisionTicket(Uuid);

impl fmt::Debug for RevisionTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RevisionTicket(..)")
    }
}

#[cfg(test)]
impl RevisionTicket {
    fn expose_for_test(self) -> Uuid {
        self.0
    }
}

#[derive(Clone, Copy)]
struct RevisionRecord {
    scope: RevisionScope,
    fingerprint: [u8; 32],
}

#[derive(Default)]
struct RevisionLedgerState {
    records: HashMap<RevisionTicket, RevisionRecord>,
    order_by_scope: HashMap<RevisionScope, VecDeque<RevisionTicket>>,
}

pub(crate) struct RevisionLedger {
    capacity_per_scope: usize,
    inner: Mutex<RevisionLedgerState>,
}

impl RevisionLedger {
    pub(crate) fn with_capacity(capacity_per_scope: usize) -> Self {
        Self {
            capacity_per_scope,
            inner: Mutex::new(RevisionLedgerState::default()),
        }
    }

    pub(crate) fn issue(&self, scope: RevisionScope, fingerprint: [u8; 32]) -> RevisionTicket {
        let mut state = self.lock_state();
        let ticket = loop {
            let candidate = RevisionTicket(Uuid::new_v4());
            if !state.records.contains_key(&candidate) {
                break candidate;
            }
        };

        state
            .records
            .insert(ticket, RevisionRecord { scope, fingerprint });
        state
            .order_by_scope
            .entry(scope)
            .or_default()
            .push_back(ticket);

        let evicted = {
            let order = state.order_by_scope.entry(scope).or_default();
            let mut evicted = Vec::new();
            while order.len() > self.capacity_per_scope {
                if let Some(oldest) = order.pop_front() {
                    evicted.push(oldest);
                }
            }
            evicted
        };
        for oldest in evicted {
            state.records.remove(&oldest);
        }

        ticket
    }

    pub(crate) fn take(&self, ticket: RevisionTicket, scope: RevisionScope) -> Option<[u8; 32]> {
        let mut state = self.lock_state();
        let record = state.records.get(&ticket).copied()?;
        if record.scope != scope {
            return None;
        }

        state.records.remove(&ticket);
        if let Some(order) = state.order_by_scope.get_mut(&scope) {
            order.retain(|candidate| *candidate != ticket);
        }
        Some(record.fingerprint)
    }

    pub(crate) fn peek(&self, ticket: RevisionTicket, scope: RevisionScope) -> Option<[u8; 32]> {
        self.lock_state()
            .records
            .get(&ticket)
            .filter(|record| record.scope == scope)
            .map(|record| record.fingerprint)
    }

    fn lock_state(&self) -> MutexGuard<'_, RevisionLedgerState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for RevisionLedger {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY_PER_SCOPE)
    }
}

impl fmt::Debug for RevisionLedger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock_state();
        formatter
            .debug_struct("RevisionLedger")
            .field("capacity_per_scope", &self.capacity_per_scope)
            .field("live_ticket_count", &state.records.len())
            .finish()
    }
}

pub(crate) fn scoped_fingerprint(domain: &'static [u8], value: &impl Serialize) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update([0]);
    hasher.update(serde_json::to_vec(value).expect("canonical revision value serializes"));
    hasher.finalize().into()
}

pub(crate) fn stepwise_fingerprint(settings: &BackendSettings) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(STEPWISE_FINGERPRINT_DOMAIN);
    hasher.update([0]);
    hash_field(
        &mut hasher,
        &[u8::from(settings.codex_app_stepwise_enabled)],
    );
    hash_field(
        &mut hasher,
        &[u8::from(settings.codex_app_stepwise_direct_send)],
    );
    hash_field(&mut hasher, settings.codex_app_stepwise_base_url.as_bytes());
    hash_field(&mut hasher, settings.codex_app_stepwise_api_key.as_bytes());
    hash_field(
        &mut hasher,
        settings.codex_app_stepwise_api_key_env.as_bytes(),
    );
    hash_field(&mut hasher, settings.codex_app_stepwise_model.as_bytes());
    hash_field(
        &mut hasher,
        &settings.codex_app_stepwise_max_items.to_le_bytes(),
    );
    hash_field(
        &mut hasher,
        &settings.codex_app_stepwise_max_input_chars.to_le_bytes(),
    );
    hash_field(
        &mut hasher,
        &settings.codex_app_stepwise_max_output_tokens.to_le_bytes(),
    );
    hash_field(
        &mut hasher,
        &settings.codex_app_stepwise_timeout_ms.to_le_bytes(),
    );
    hasher.finalize().into()
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use codex_plus_core::settings::BackendSettings;

    use super::{RevisionLedger, RevisionScope, scoped_fingerprint, stepwise_fingerprint};

    #[test]
    fn tickets_are_scoped_single_use_bounded_and_opaque() {
        let ledger = RevisionLedger::with_capacity(2);
        let first = ledger.issue(RevisionScope::Stepwise, [1; 32]);
        let second = ledger.issue(RevisionScope::Stepwise, [2; 32]);
        let overlay = ledger.issue(RevisionScope::ImageOverlay, [4; 32]);
        let third = ledger.issue(RevisionScope::Stepwise, [3; 32]);

        assert_eq!(ledger.take(first, RevisionScope::Stepwise), None);
        assert_eq!(ledger.take(second, RevisionScope::ImageOverlay), None);
        assert_eq!(ledger.peek(second, RevisionScope::Stepwise), Some([2; 32]));
        assert_eq!(ledger.take(second, RevisionScope::Stepwise), Some([2; 32]));
        assert_eq!(ledger.take(second, RevisionScope::Stepwise), None);
        assert_eq!(ledger.take(third, RevisionScope::Stepwise), Some([3; 32]));
        assert_eq!(
            ledger.take(overlay, RevisionScope::ImageOverlay),
            Some([4; 32])
        );

        let ticket_debug = format!("{third:?}");
        assert!(!ticket_debug.contains(&third.expose_for_test().to_string()));
        assert!(!format!("{ledger:?}").contains(&hex([3; 32])));
    }

    fn hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn fingerprints_are_domain_separated_and_stepwise_specific() {
        assert_ne!(
            scoped_fingerprint(b"app-path/v1", &"same"),
            scoped_fingerprint(b"image-overlay/v1", &"same")
        );

        let original = BackendSettings::default();
        let mut changed_key = original.clone();
        changed_key.codex_app_stepwise_api_key = "replacement-key".to_owned();
        let mut unrelated = original.clone();
        unrelated.codex_app_path = "unrelated".to_owned();

        assert_ne!(
            stepwise_fingerprint(&original),
            stepwise_fingerprint(&changed_key)
        );
        assert_eq!(
            stepwise_fingerprint(&original),
            stepwise_fingerprint(&unrelated)
        );
    }
}
