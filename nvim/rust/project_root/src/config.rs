use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Dictionary, String as NvimString};
use project_root_core::{RootIndicators, default_root_indicators, root_indicators_from_vec};

#[derive(Debug, Clone)]
pub struct ProjectRootConfig {
    pub root_indicators: RootIndicators,
}

impl ProjectRootConfig {
    fn parse_root_indicators(config: &Dictionary) -> Option<RootIndicators> {
        let key = NvimString::from("root_indicators");
        let value = config.get(&key)?;
        let values = Vec::<NvimString>::from_object(value.clone()).ok()?;
        let strings: Vec<String> = values
            .into_iter()
            .map(|val| val.to_string_lossy().into_owned())
            .filter(|val| !val.is_empty())
            .collect();
        root_indicators_from_vec(strings)
    }

    pub fn from_dict(config: Option<&Dictionary>) -> Self {
        let root_indicators = config
            .and_then(Self::parse_root_indicators)
            .map_or_else(default_root_indicators, |indicators| indicators);
        Self { root_indicators }
    }
}
