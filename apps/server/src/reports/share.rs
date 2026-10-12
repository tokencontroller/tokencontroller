//! Shares of the period (docs: Report > Reading reports): each line's part
//! of the report's total in percent with two decimals, for seat plans where
//! usage is not charged in money. Lines that are all zero or more add up to
//! exactly 100.00 by the largest remainder method.

/// Basis points in a whole.
const WHOLE: i128 = 10_000;

/// Each amount's share of their sum, such as `41.20`. All `None` when the
/// sum is zero or less.
#[must_use]
pub fn percentages(amounts: &[i64]) -> Vec<Option<String>> {
    basis_points(amounts)
        .into_iter()
        .map(|points| points.map(format_points))
        .collect()
}

/// Each amount's share of their sum in hundredths of a percent.
fn basis_points(amounts: &[i64]) -> Vec<Option<i128>> {
    let total = amounts
        .iter()
        .fold(0_i128, |sum, amount| sum.saturating_add(i128::from(*amount)));
    if total <= 0 {
        return vec![None; amounts.len()];
    }
    let parts: Vec<(i128, i128)> = amounts
        .iter()
        .map(|amount| {
            let scaled = i128::from(*amount).saturating_mul(WHOLE);
            (scaled.div_euclid(total), scaled.rem_euclid(total))
        })
        .collect();
    let mut points: Vec<i128> = parts.iter().map(|(quotient, _)| *quotient).collect();
    if amounts.iter().all(|amount| *amount >= 0) {
        let given = points.iter().fold(0_i128, |sum, part| sum.saturating_add(*part));
        let mut order: Vec<usize> = (0..parts.len()).collect();
        order.sort_by(|a, b| {
            let remainder = |index: &usize| parts.get(*index).map_or(0, |(_, remainder)| *remainder);
            remainder(b).cmp(&remainder(a)).then_with(|| a.cmp(b))
        });
        let missing = usize::try_from(WHOLE.saturating_sub(given)).unwrap_or(0);
        for index in order.into_iter().take(missing) {
            if let Some(slot) = points.get_mut(index) {
                *slot = slot.saturating_add(1);
            }
        }
    } else {
        // With negative lines, such as a credit adjustment, round each share
        // half up on its own: they still add up to about 100.
        for (slot, (_, remainder)) in points.iter_mut().zip(&parts) {
            if remainder.saturating_mul(2) >= total {
                *slot = slot.saturating_add(1);
            }
        }
    }
    points.into_iter().map(Some).collect()
}

/// `4120` as `41.20`, `-50` as `-0.50`.
fn format_points(points: i128) -> String {
    let sign = if points < 0 { "-" } else { "" };
    let points = points.unsigned_abs();
    format!("{sign}{}.{:02}", points.div_euclid(100), points.rem_euclid(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shares(amounts: &[i64]) -> Vec<String> {
        percentages(amounts).into_iter().map(Option::unwrap_or_default).collect()
    }

    #[test]
    fn shares_add_up_to_a_hundred() {
        assert_eq!(shares(&[1, 1, 1]), ["33.34", "33.33", "33.33"]);
        assert_eq!(shares(&[3, 1]), ["75.00", "25.00"]);
        assert_eq!(shares(&[5, 0]), ["100.00", "0.00"]);
        assert_eq!(shares(&[2, 1, 1, 0, 3]), ["28.57", "14.29", "14.28", "0.00", "42.86"]);
    }

    #[test]
    fn nothing_to_share_gives_no_shares() {
        assert_eq!(percentages(&[0, 0]), [None, None]);
        assert_eq!(percentages(&[-5, 2]), [None, None]);
        assert!(percentages(&[]).is_empty());
    }

    #[test]
    fn negative_lines_get_negative_shares() {
        assert_eq!(shares(&[110, -10]), ["110.00", "-10.00"]);
        assert_eq!(shares(&[201, -1]), ["100.50", "-0.50"]);
    }
}
