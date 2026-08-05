//! 内嵌静态资源 (Materialize v2.3.3, materializecss/materialize 社区维护分支).

pub const MATERIALIZE_CSS: &str = include_str!("../assets/materialize.min.css");
pub const MATERIALIZE_JS: &str = include_str!("../assets/materialize.min.js");
pub const THEME_CSS: &str = include_str!("../assets/chapbook-theme.css");
