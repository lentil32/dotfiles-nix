use crate::core::RootIndicators;
use crate::core::default_root_indicators;
use crate::core::root_indicators_from_vec;
use nvim_oxi::Dictionary;
use nvim_oxi::String as NvimString;
use nvimrs_nvim_oxi_utils::decode;

#[derive(Debug, Clone)]
pub struct ProjectRootConfig {
    pub root_indicators: RootIndicators,
}

impl ProjectRootConfig {
    fn parse_root_indicators(config: &Dictionary) -> Option<RootIndicators> {
        let value = decode::get_object(config, "root_indicators")?;
        let values =
            decode::parse_from_object::<Vec<NvimString>>(value, "root_indicators", "array[string]")
                .ok()?;
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
            .unwrap_or_else(default_root_indicators);
        Self { root_indicators }
    }
}
