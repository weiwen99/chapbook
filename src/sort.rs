//! 排序模型: `?sort=Column:Order` 查询参数.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl fmt::Display for SortOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SortOrder::Asc => f.write_str("Asc"),
            SortOrder::Desc => f.write_str("Desc"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Type,
    LastModified,
    LastAccess,
    Creation,
}

impl SortColumn {
    pub const ALL: [SortColumn; 6] = [
        SortColumn::Name,
        SortColumn::Type,
        SortColumn::Size,
        SortColumn::LastModified,
        SortColumn::LastAccess,
        SortColumn::Creation,
    ];
}

impl fmt::Display for SortColumn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SortColumn::Name => "Name",
            SortColumn::Size => "Size",
            SortColumn::Type => "Type",
            SortColumn::LastModified => "LastModified",
            SortColumn::LastAccess => "LastAccess",
            SortColumn::Creation => "Creation",
        };
        f.write_str(s)
    }
}

impl FromStr for SortColumn {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Name" => Ok(SortColumn::Name),
            "Size" => Ok(SortColumn::Size),
            "Type" => Ok(SortColumn::Type),
            "LastModified" => Ok(SortColumn::LastModified),
            "LastAccess" => Ok(SortColumn::LastAccess),
            "Creation" => Ok(SortColumn::Creation),
            other => Err(format!("unknown SortColumn: {other}")),
        }
    }
}

impl FromStr for SortOrder {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Asc" => Ok(SortOrder::Asc),
            "Desc" => Ok(SortOrder::Desc),
            other => Err(format!("unknown SortOrder: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortBy {
    pub column: SortColumn,
    pub order: SortOrder,
}

impl Default for SortBy {
    fn default() -> Self {
        SortBy {
            column: SortColumn::Name,
            order: SortOrder::Asc,
        }
    }
}

impl FromStr for SortBy {
    type Err = String;

    /// 解析 `Column:Order` 格式.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.as_slice() {
            [column, order] => {
                let column: SortColumn = column.parse()?;
                let order: SortOrder = order.parse()?;
                Ok(SortBy { column, order })
            }
            _ => Err("Invalid SortBy format".to_string()),
        }
    }
}
