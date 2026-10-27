use super::{Fix, Indicator, Mode, Site, SpaceSite};
use super::{SpaceEdit, input::ExplicitKey};
use crate::rules::support::span_utils::BytePos;

pub(super) fn separate(
    initial: ExplicitKey,
    (from, to, count): SpaceEdit,
) -> Option<SpaceEdit> {
    if initial.key && count == 0 {
        (!initial.single_space).then_some((from, to, 1))
    } else {
        Some((from, to, count))
    }
}

pub(super) fn allowed(site: &Site, tolerance: i64) -> Option<usize> {
    let tolerance = usize::try_from(tolerance).ok()?;
    Some(if site.before {
        tolerance.max(site.min)
    } else {
        tolerance
    })
}

pub(super) fn through(sites: &[Site], colons: bool, hyphens: bool) -> Vec<SpaceSite> {
    sites
        .iter()
        .filter_map(|site| {
            let enabled = if site.indicator == Indicator::Dash {
                hyphens
            } else {
                colons
            };
            enabled
                .then(|| space_site(Mode::Format, site, i64::from(!site.before)))
                .flatten()
                .map(|edit| SpaceSite {
                    indicator: site.indicator,
                    edit,
                })
        })
        .collect()
}
pub(super) fn space_site(mode: Mode, site: &Site, tolerance: i64) -> Option<SpaceEdit> {
    if site.fix != Fix::Safe {
        return None;
    }
    let target = usize::try_from(tolerance).ok()?.max(site.min);
    let edit = match mode {
        Mode::Lint => site.spaces > target,
        Mode::Format => site.ws.len() != target || site.contains_tab,
    };
    edit.then(|| {
        (
            BytePos::new(site.ws.start),
            BytePos::new(site.ws.end),
            target,
        )
    })
}

pub(super) fn extent(
    initial: &super::input::Extent,
    depth: usize,
    line: usize,
) -> Option<Fix> {
    use super::input::ExtentKind;
    let multiline = depth > 0
        && match initial.kind {
            ExtentKind::Comment | ExtentKind::Close => false,
            ExtentKind::BlockScalar => initial.last > line,
            _ => initial.first > line,
        };
    if multiline {
        Some(Fix::Reindents)
    } else if depth == 0 {
        Some(Fix::Safe)
    } else {
        None
    }
}

pub(super) fn compact(initial: &super::input::Extent, line: usize) -> bool {
    matches!(initial.kind, super::input::ExtentKind::Open) && initial.first == line
}

pub(super) const fn minimum(initial: &super::input::Adjacent) -> Option<usize> {
    match initial {
        super::input::Adjacent::Value => Some(0),
        super::input::Adjacent::Property => Some(1),
        super::input::Adjacent::Other => None,
    }
}

pub(super) const fn after(initial: super::input::Following) -> Option<Fix> {
    match initial {
        super::input::Following::BreakOrComment => None,
        super::input::Following::Separator => Some(Fix::Elsewhere),
        super::input::Following::Content => Some(Fix::Safe),
    }
}
