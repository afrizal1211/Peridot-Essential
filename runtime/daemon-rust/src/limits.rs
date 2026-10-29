//! A successful write is an accepted request, not proof of the effective limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bound {
    Min,
    Max,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Readback {
    pub min: i64,
    pub max: i64,
    pub exact: bool,
}
pub(super) fn classify(min: i64, max: i64, actual: (i64, i64)) -> Result<Readback, String> {
    if min <= 0 || min > max {
        return Err("invalid requested frequency range".into());
    }
    if actual.0 <= 0 || actual.0 > actual.1 {
        return Err(format!(
            "invalid effective frequency range {}-{}",
            actual.0, actual.1
        ));
    }
    Ok(Readback {
        min: actual.0,
        max: actual.1,
        exact: actual == (min, max),
    })
}
/// Roll back only IO/readback failures. A valid adjusted range remains visible.
pub(super) fn submit(
    min: i64,
    max: i64,
    old: (i64, i64),
    mut put: impl FnMut(Bound, i64) -> Result<(), String>,
    mut read: impl FnMut() -> Result<(i64, i64), String>,
) -> Result<Readback, String> {
    classify(min, max, old)?;
    let attempt = (|| {
        if min > old.1 {
            put(Bound::Max, max)?;
            put(Bound::Min, min)?;
        } else {
            put(Bound::Min, min)?;
            put(Bound::Max, max)?;
        }
        classify(min, max, read()?)
    })();
    if let Err(error) = attempt {
        let mut failures = Vec::new();
        for (bound, value) in [
            (Bound::Min, old.0.min(min)),
            (Bound::Max, old.1),
            (Bound::Min, old.0),
        ] {
            if let Err(e) = put(bound, value) {
                failures.push(e);
            }
        }
        return Err(if failures.is_empty() {
            error
        } else {
            format!("{error}; rollback incomplete: {}", failures.join("; "))
        });
    }
    attempt
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn cpu_3014400_request_clamped_to_2323200_is_not_reported_exact() {
        let live = RefCell::new((480000, 2323200));
        let writes = RefCell::new(Vec::new());
        let old = *live.borrow();
        let status = submit(
            3014400,
            3014400,
            old,
            |bound, value| {
                writes.borrow_mut().push((bound, value));
                let mut live = live.borrow_mut();
                match bound {
                    Bound::Min => live.0 = value.min(live.1),
                    Bound::Max => live.1 = value.min(2323200),
                }
                Ok(())
            },
            || Ok(*live.borrow()),
        )
        .unwrap();
        assert_eq!(
            status,
            Readback {
                min: 2323200,
                max: 2323200,
                exact: false
            }
        );
        assert_eq!(
            *writes.borrow(),
            vec![(Bound::Max, 3014400), (Bound::Min, 3014400)]
        );
    }
    #[test]
    fn gpu_adjusted_maximum_does_not_trigger_rollback() {
        let writes = RefCell::new(Vec::new());
        let status = submit(
            264000000,
            1150000000,
            (264000000, 510000000),
            |b, v| {
                writes.borrow_mut().push((b, v));
                Ok(())
            },
            || Ok((264000000, 763000000)),
        )
        .unwrap();
        assert!(!status.exact);
        assert_eq!(writes.borrow().len(), 2);
    }
    #[test]
    fn exact_and_external_changes_are_distinct_from_invalid_reads() {
        assert!(classify(264, 1150, (264, 1150)).unwrap().exact);
        assert!(!classify(264, 510, (264, 763)).unwrap().exact);
        assert!(classify(264, 1150, (0, 1150)).is_err());
        assert!(classify(264, 1150, (1150, 510)).is_err());
    }
    #[test]
    fn permission_failure_is_error_and_attempts_original_restore() {
        let writes = RefCell::new(Vec::new());
        let result = submit(
            264,
            1150,
            (264, 510),
            |b, v| {
                writes.borrow_mut().push((b, v));
                if b == Bound::Max && v == 1150 {
                    Err("Permission denied".into())
                } else {
                    Ok(())
                }
            },
            || Ok((264, 510)),
        );
        assert!(result.unwrap_err().contains("Permission denied"));
        assert_eq!(writes.borrow().last(), Some(&(Bound::Min, 264)));
    }
    #[test]
    fn unreadable_state_and_failed_rollback_are_not_silenced() {
        let result = submit(
            264,
            1150,
            (264, 510),
            |_, _| Err("Read-only filesystem".into()),
            || Err("unreadable".into()),
        );
        assert!(result.unwrap_err().contains("rollback incomplete"));
    }
}
