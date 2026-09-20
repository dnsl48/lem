//! Decode the frames captured from a real editor in Task 1.
//!
//! Counts are exact rather than `> 0` so a regression that silently drops
//! a variant is caught rather than merely tolerated.

use lem_protocol::{Bulk, Instruction};

fn instructions() -> Vec<Instruction> {
    include_str!("fixtures/frame.jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let message: serde_json::Value = serde_json::from_str(line).unwrap();
            (message["method"] == "bulk").then(|| message["params"].clone())
        })
        .flat_map(|params| serde_json::from_value::<Bulk>(params).unwrap())
        .map(|raw| raw.parse().expect("every instruction should decode"))
        .collect()
}

#[test]
fn every_instruction_in_the_capture_decodes() {
    let all = instructions();
    assert_eq!(all.len(), 133, "total instructions");

    let count = |f: &dyn Fn(&Instruction) -> bool| all.iter().filter(|i| f(i)).count();

    assert_eq!(count(&|i| matches!(i, Instruction::MakeView(_))), 2);
    assert_eq!(count(&|i| matches!(i, Instruction::Put(_))), 21);
    assert_eq!(count(&|i| matches!(i, Instruction::ModelinePut(_))), 59);
    assert_eq!(count(&|i| matches!(i, Instruction::ClearEol(_))), 22);
    assert_eq!(count(&|i| matches!(i, Instruction::ClearEob(_))), 2);
    assert_eq!(count(&|i| matches!(i, Instruction::MoveCursor(_))), 7);
    assert_eq!(count(&|i| matches!(i, Instruction::ResizeView(_))), 1);
    assert_eq!(count(&|i| matches!(i, Instruction::MoveView(_))), 1);
}

#[test]
fn only_the_deliberately_unhandled_methods_fall_through() {
    let mut unhandled: Vec<String> = instructions()
        .into_iter()
        .filter_map(|i| match i {
            Instruction::Other { method } => Some(method),
            _ => None,
        })
        .collect();
    unhandled.sort();
    unhandled.dedup();
    assert_eq!(
        unhandled,
        vec!["change-view", "redraw-view-after", "update-display"],
        "an unexpected method here means the protocol grew something we ignore silently"
    );
}
