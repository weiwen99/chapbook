//! SortBy 查询解析测试.

use chapbook::sort::{SortBy, SortColumn, SortOrder};

#[test]
fn parse_valid_string() {
    assert_eq!(
        "Name:Asc".parse::<SortBy>(),
        Ok(SortBy {
            column: SortColumn::Name,
            order: SortOrder::Asc
        })
    );
    assert_eq!(
        "Name:Desc".parse::<SortBy>(),
        Ok(SortBy {
            column: SortColumn::Name,
            order: SortOrder::Desc
        })
    );
    assert_eq!(
        "LastModified:Desc".parse::<SortBy>(),
        Ok(SortBy {
            column: SortColumn::LastModified,
            order: SortOrder::Desc
        })
    );
}

#[test]
fn parse_invalid_string() {
    assert!("Name:Invalid".parse::<SortBy>().is_err());
    assert!("Name:Invalid:Ext".parse::<SortBy>().is_err());
}

/// 查询格式大小写敏感: 只有精确的 `Column:Order` 合法 (URL 契约来自 spec Q10/Q19).
#[test]
fn parse_is_case_sensitive_and_exact() {
    for invalid in [
        "name:Asc",
        "Name:asc",
        "NAME:ASC",
        "name:desc",
        "Name",
        "Asc",
        "Name:",
        ":Asc",
        " Name:Asc",
        "Name:Asc ",
        "Name:Asc:",
    ] {
        assert!(
            invalid.parse::<SortBy>().is_err(),
            "{invalid:?} must not parse"
        );
    }
}

/// 缺省排序: Name:Asc — 与显式 `Name:Asc` 参数值同一状态 (Q19/Q24).
#[test]
fn default_is_name_asc() {
    assert_eq!(
        SortBy::default(),
        SortBy {
            column: SortColumn::Name,
            order: SortOrder::Asc
        }
    );
    assert_eq!("Name:Asc".parse::<SortBy>(), Ok(SortBy::default()));
}
