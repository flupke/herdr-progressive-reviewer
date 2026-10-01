//! Jev's significance judgements about a comparison's changes, and the classifier
//! that makes them.

mod classifier;
mod ledger;

pub use classifier::{JevClassifier, SignificanceClassifier, SignificancePlan};
pub use ledger::{
    ChangeInventory, ChangeUnit, ExcludedLines, Significance, SignificanceLedger,
    SignificanceResult,
};
