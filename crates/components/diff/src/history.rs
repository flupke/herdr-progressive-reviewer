use ui_events::ReviewLocation;

const MAXIMUM_JUMP_COUNT: usize = 100;

#[derive(Clone, Debug, Default)]
pub(super) struct LocationHistory {
    older: Vec<ReviewLocation>,
    newer: Vec<ReviewLocation>,
}

#[derive(Clone, Copy)]
pub(super) enum LocationHistoryDirection {
    Previous,
    Next,
}

impl LocationHistory {
    pub(super) fn record(&mut self, origin: ReviewLocation, target: &ReviewLocation) {
        if origin.same_line(target) {
            return;
        }
        self.older.extend(self.newer.drain(..).rev());
        self.older.retain(|location| !location.same_line(&origin));
        self.older.push(origin);
        if self.older.len() > MAXIMUM_JUMP_COUNT {
            self.older.remove(0);
        }
    }

    pub(super) fn navigate(
        &mut self,
        direction: LocationHistoryDirection,
        current: ReviewLocation,
        is_restorable: impl Fn(&ReviewLocation) -> bool,
    ) -> Option<ReviewLocation> {
        let (source, destination) = match direction {
            LocationHistoryDirection::Previous => (&mut self.older, &mut self.newer),
            LocationHistoryDirection::Next => (&mut self.newer, &mut self.older),
        };
        while let Some(target) = source.pop() {
            if is_restorable(&target) {
                if is_restorable(&current) {
                    destination.push(current);
                }
                return Some(target);
            }
        }
        None
    }
}
