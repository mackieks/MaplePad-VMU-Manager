use crate::vmu::VmuImage;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Name,
    VmDescription,
    DcDescription,
    Blocks,
    Type,
    Copy,
    Created,
    FirstBlock,
    Crc,
}

pub const COLUMNS: [(Column, &str, f32); 9] = [
    (Column::Name, "Name", 182.0),
    (Column::VmDescription, "VM Description", 180.0),
    (Column::DcDescription, "DC Description", 280.0),
    (Column::Blocks, "Blocks", 70.0),
    (Column::Type, "Type", 66.0),
    (Column::Copy, "Copy", 62.0),
    (Column::Created, "Created", 174.0),
    (Column::FirstBlock, "First Block", 95.0),
    (Column::Crc, "CRC", 70.0),
];

// Numeric runs compare numerically without integer overflow (SAVE2 before SAVE10).
fn natural(a: &str, b: &str) -> Ordering {
    let a = a.to_lowercase();
    let b = b.to_lowercase();
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let x: String = std::iter::from_fn(|| a.next_if(|c| c.is_ascii_digit())).collect();
                let y: String = std::iter::from_fn(|| b.next_if(|c| c.is_ascii_digit())).collect();
                let (xn, yn) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = xn
                    .len()
                    .cmp(&yn.len())
                    .then_with(|| xn.cmp(yn))
                    .then_with(|| x.len().cmp(&y.len()));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                a.next();
                b.next();
                let order = x.cmp(&y);
                if order != Ordering::Equal {
                    return order;
                }
            }
            (x, y) => return x.cmp(&y),
        }
    }
}

pub fn indices(image: &VmuImage, column: Column, descending: bool) -> Vec<usize> {
    let mut order: Vec<_> = (0..image.files.len()).collect();
    order.sort_by(|&a, &b| {
        let (a_file, b_file) = (&image.files[a], &image.files[b]);
        let cmp = match column {
            Column::Name => natural(&a_file.name, &b_file.name),
            Column::VmDescription => natural(&a_file.vm_description, &b_file.vm_description),
            Column::DcDescription => natural(&a_file.dc_description, &b_file.dc_description),
            Column::Blocks => a_file.blocks.cmp(&b_file.blocks),
            Column::Type => a_file.kind.cmp(b_file.kind),
            Column::Copy => a_file.copy_protected.cmp(&b_file.copy_protected),
            Column::Created => a_file.created.cmp(&b_file.created),
            Column::FirstBlock => a_file.first_block.cmp(&b_file.first_block),
            Column::Crc => a_file.crc.cmp(&b_file.crc),
        }
        .then_with(|| natural(&a_file.name, &b_file.name))
        .then(a.cmp(&b));
        if descending {
            cmp.reverse()
        } else {
            cmp
        }
    });
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural_sort_handles_numeric_runs_unicode_and_leading_zeros() {
        assert_eq!(natural("Save2", "SAVE10"), Ordering::Less);
        assert_eq!(natural("SAVE002", "SAVE2"), Ordering::Greater);
        assert_eq!(natural("保存2", "保存10"), Ordering::Less);
        assert_eq!(natural("Abc", "aBC"), Ordering::Equal);
    }
}
