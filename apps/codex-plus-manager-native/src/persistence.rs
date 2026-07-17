use crate::i18n::{Locale, ThemeMode};

pub const STORAGE_KEY: &str = "native_manager_ui";

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PersistedUiState {
    pub locale: Locale,
    pub theme: ThemeMode,
}

pub fn load(storage: Option<&dyn eframe::Storage>) -> PersistedUiState {
    storage
        .and_then(|storage| eframe::get_value(storage, STORAGE_KEY))
        .unwrap_or_default()
}

pub fn save(storage: &mut dyn eframe::Storage, state: &PersistedUiState) {
    eframe::set_value(storage, STORAGE_KEY, state);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::i18n::{Locale, ThemeMode};
    use eframe::Storage;

    use super::*;

    #[derive(Default)]
    struct MemoryStorage {
        values: HashMap<String, String>,
    }

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.values.remove(key);
        }

        fn flush(&mut self) {}
    }

    #[test]
    fn persisted_ui_defaults_to_chinese_dark_and_round_trips() {
        let defaults = PersistedUiState::default();
        assert_eq!(defaults.locale, Locale::ZhCn);
        assert_eq!(defaults.theme, ThemeMode::Dark);

        let encoded = serde_json::to_string(&PersistedUiState {
            locale: Locale::En,
            theme: ThemeMode::Light,
        })
        .unwrap();
        let decoded: PersistedUiState = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.locale, Locale::En);
        assert_eq!(decoded.theme, ThemeMode::Light);
    }

    #[test]
    fn eframe_storage_round_trip_and_corrupt_state_fallback() {
        let mut storage = MemoryStorage::default();
        let expected = PersistedUiState {
            locale: Locale::En,
            theme: ThemeMode::Light,
        };

        save(&mut storage, &expected);
        assert_eq!(load(Some(&storage)), expected);

        storage.set_string(STORAGE_KEY, "not valid ron".to_owned());
        assert_eq!(load(Some(&storage)), PersistedUiState::default());
        assert_eq!(load(None), PersistedUiState::default());
    }
}
