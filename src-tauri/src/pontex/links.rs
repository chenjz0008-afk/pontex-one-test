//! 外部链接。唯一合法的形式是 `pontex-one://skill/install?slug=<slug>`。
//!
//! 链接里不允许出现地址、命令、配置或 Key：网页能做的只有「请求打开某个技能的确认窗」。
//! 上游的 `ccswitch://` 解析器不接外部输入。

use super::consts::SCHEME;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    InstallSkill { slug: String },
}

pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// 按字面匹配，不做 URL 解码和规范化：任何多余的东西都整条拒绝。
pub fn parse(raw: &str) -> Option<Link> {
    let slug = raw
        .strip_prefix(SCHEME)?
        .strip_prefix("://skill/install?slug=")?;
    valid_slug(slug).then(|| Link::InstallSkill { slug: slug.into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_one_shape() {
        assert_eq!(
            parse("pontex-one://skill/install?slug=brand-kit2"),
            Some(Link::InstallSkill { slug: "brand-kit2".into() })
        );
        for bad in [
            "",
            "ccswitch://v1/import?resource=provider&app=claude&name=x&endpoint=https://evil.test&apiKey=k",
            "pontex-one://v1/import?resource=provider&app=claude",
            "pontex-one://skill/install?slug=",
            "pontex-one://skill/install?slug=a&url=https://evil.test",
            "pontex-one://skill/install?slug=a#x",
            "pontex-one://skill/install?slug=../a",
            "pontex-one://skill/install?slug=a%2Fb",
            "pontex-one://skill/install?slug=A",
            "pontex-one://skill/install?slug=-a",
            "pontex-one://skill/install?slug=a--b",
            "pontex-one://skill/install?slug=a ",
            "pontex-one://skill/install?url=x&slug=a",
            "pontex-one://u:p@skill/install?slug=a",
            "pontex-one://skill:80/install?slug=a",
            "PONTEX-ONE://skill/install?slug=a",
            "https://skill/install?slug=a",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert!(!valid_slug(&"a".repeat(65)));
    }
}
