//! Edits are transactional in memory; disk/flash writes happen only on Save Changes.
use super::*;

fn directory_offsets() -> impl Iterator<Item = usize> {
    (241..=253)
        .rev()
        .flat_map(|b| (0..512).step_by(32).map(move |n| b * 512 + n))
}

fn chains(image: &VmuImage) -> Result<Vec<Vec<usize>>, String> {
    let mut claimed = vec![false; image.capacity];
    let mut result = Vec::new();
    for at in directory_offsets() {
        let entry = &image.bytes[at..at + 32];
        if !matches!(entry[0], 0 | 0xff | 0x33 | 0xcc) {
            return Err("This VMU has an unknown directory entry; editing is disabled.".into());
        }
    }
    for file in &image.files {
        let mut block = usize::from(file.first_block);
        let mut chain = Vec::new();
        if file.blocks == 0 || file.bytes.len() != usize::from(file.blocks) * BLOCK_SIZE {
            return Err(format!("{} has an invalid block chain.", file.name));
        }
        for n in 0..usize::from(file.blocks) {
            if block >= image.capacity || claimed[block] {
                return Err(format!(
                    "{} has an invalid or shared block chain.",
                    file.name
                ));
            }
            claimed[block] = true;
            chain.push(block);
            let next = fat_entry(&image.bytes, block);
            if n + 1 == usize::from(file.blocks) && next != 0xfffa {
                return Err(format!("{} has an invalid end-of-chain marker.", file.name));
            }
            block = usize::from(next);
        }
        result.push(chain);
    }
    Ok(result)
}

fn set_fat(bytes: &mut [u8], block: usize, value: u16) {
    bytes[FAT_OFFSET + block * 2..FAT_OFFSET + block * 2 + 2].copy_from_slice(&value.to_le_bytes());
}

fn finish(
    image: &VmuImage,
    mut bytes: Vec<u8>,
    entries: Vec<[u8; 32]>,
) -> Result<VmuImage, String> {
    for (index, offset) in directory_offsets().enumerate() {
        bytes[offset..offset + 32].copy_from_slice(entries.get(index).unwrap_or(&[0; 32]));
    }
    let mut updated = parse_image(&bytes)?;
    updated.format = image.format;
    updated.original_format = image.original_format.or(Some(image.format));
    updated.original_bytes = Some(
        image
            .original_bytes
            .as_ref()
            .unwrap_or(&image.bytes)
            .clone(),
    );
    // Deleting a just-pasted file can return exactly to the initial image only if
    // allocation bytes also match. Otherwise retain the pending-change indicator.
    Ok(updated)
}

pub fn delete(image: &VmuImage, index: usize) -> Result<VmuImage, String> {
    let chains = chains(image)?;
    let chain = chains.get(index).ok_or("Select a file to delete.")?;
    let mut bytes = image.bytes.clone();
    for &block in chain {
        set_fat(&mut bytes, block, FAT_FREE);
    }
    let entries = image
        .files
        .iter()
        .enumerate()
        .filter(|(n, _)| *n != index)
        .map(|(_, f)| f.directory_entry)
        .collect();
    finish(image, bytes, entries)
}

pub fn copy(image: &VmuImage, index: usize) -> Result<VmuFile, String> {
    chains(image)?;
    image
        .files
        .get(index)
        .cloned()
        .ok_or("Select a file to copy.".into())
}

/// Replace one file atomically, reusing its blocks before taking free blocks.
pub fn replace(image: &VmuImage, index: usize, file: &VmuFile) -> Result<VmuImage, String> {
    let chains = chains(image)?;
    let old = chains.get(index).ok_or("Icon file no longer exists")?;
    let needed = usize::from(file.blocks);
    if needed == 0 || file.bytes.len() != needed * BLOCK_SIZE {
        return Err("Invalid replacement icon size".into());
    }
    if needed > old.len() + image.free {
        return Err(format!("Not enough free blocks: icon needs {needed}; {} available including its existing blocks.", old.len()+image.free));
    }
    let mut blocks: Vec<_> = old.iter().copied().take(needed).collect();
    for block in (0..image.capacity).rev() {
        if blocks.len() == needed {
            break;
        }
        if fat_entry(&image.bytes, block) == FAT_FREE {
            blocks.push(block);
        }
    }
    let mut bytes = image.bytes.clone();
    for &block in old {
        set_fat(&mut bytes, block, FAT_FREE);
    }
    let entry = write_file(&mut bytes, file, &blocks);
    let mut entries: Vec<_> = image.files.iter().map(|f| f.directory_entry).collect();
    entries[index] = entry;
    finish(image, bytes, entries)
}

#[derive(Clone, Copy)]
pub enum Placement {
    FirstFit,
    After(usize),
}

fn free_run(bytes: &[u8], ceiling: usize, needed: usize) -> Option<Vec<usize>> {
    if needed == 0 || needed > ceiling {
        return None;
    }
    (needed..=ceiling)
        .rev()
        .find(|&end| (end - needed..end).all(|b| fat_entry(bytes, b) == FAT_FREE))
        .map(|end| (end - needed..end).rev().collect())
}

fn write_file(bytes: &mut [u8], file: &VmuFile, blocks: &[usize]) -> [u8; 32] {
    for (n, &block) in blocks.iter().enumerate() {
        bytes[block * BLOCK_SIZE..(block + 1) * BLOCK_SIZE]
            .copy_from_slice(&file.bytes[n * BLOCK_SIZE..(n + 1) * BLOCK_SIZE]);
        set_fat(
            bytes,
            block,
            blocks.get(n + 1).map_or(0xfffa, |&b| b as u16),
        );
    }
    let mut entry = file.directory_entry;
    entry[2..4].copy_from_slice(&(blocks[0] as u16).to_le_bytes());
    entry
}

fn data_order(image: &VmuImage) -> Vec<usize> {
    let mut order: Vec<_> = (0..image.files.len())
        .filter(|&i| image.files[i].kind != "GAME")
        .collect();
    order.sort_by_key(|&i| std::cmp::Reverse(image.files[i].first_block));
    order
}

// Keep the prefix byte-for-byte when possible; move only the affected suffix.
// Fragmented/interleaved chains may require normalizing the prefix too. All file
// payloads are immutable snapshots, so overlapping source/destination moves are safe.
fn ripple(
    image: &VmuImage,
    files: &[VmuFile],
    order: &[usize],
    prefix: usize,
) -> Result<VmuImage, String> {
    let old_chains = chains(image)?;
    let attempt = |keep: usize| -> Result<VmuImage, String> {
        let mut bytes = image.bytes.clone();
        let mut cursor = image.capacity;
        for &index in &order[..keep] {
            let chain = &old_chains[index];
            if *chain.iter().max().unwrap() >= cursor {
                return Err("Interleaved chains".into());
            }
            cursor = *chain.iter().min().unwrap();
        }
        for &index in &order[keep..] {
            if let Some(chain) = old_chains.get(index) {
                for &block in chain {
                    set_fat(&mut bytes, block, FAT_FREE);
                }
            }
        }
        let mut entries: Vec<_> = files.iter().map(|f| f.directory_entry).collect();
        for &index in &order[keep..] {
            let file = &files[index];
            let blocks = free_run(&bytes, cursor, usize::from(file.blocks))
                .ok_or("Cannot fit the ripple arrangement into contiguous free blocks. No changes were applied.")?;
            cursor = *blocks.last().unwrap();
            entries[index] = write_file(&mut bytes, file, &blocks);
        }
        finish(image, bytes, entries)
    };
    attempt(prefix).or_else(|error| if prefix > 0 { attempt(0) } else { Err(error) })
}

pub fn paste(image: &VmuImage, file: &VmuFile, placement: Placement) -> Result<VmuImage, String> {
    chains(image)?;
    let needed = usize::from(file.blocks);
    if needed == 0 || file.bytes.len() != needed * BLOCK_SIZE {
        return Err("The copied save has an invalid size.".into());
    }
    if image.free < needed {
        return Err(format!(
            "Not enough free blocks.\n\n{} needs {needed} blocks; this VMU has {} free.",
            file.name, image.free
        ));
    }
    if image
        .files
        .iter()
        .any(|f| f.directory_entry[4..16] == file.directory_entry[4..16])
    {
        return Err(format!(
            "{} already exists in this VMU. Delete it first to replace it.",
            file.name
        ));
    }
    if image.files.len() >= 13 * 16 {
        return Err("The VMU directory is full.".into());
    }
    let is_game = file.directory_entry[0] == 0xcc;
    if let Placement::After(anchor) = placement {
        if is_game || image.files.get(anchor).is_some_and(|f| f.kind == "GAME") {
            return Err("VMU games must remain at block 0. Use the toolbar Paste button for automatic placement.".into());
        }
        let mut order = data_order(image);
        let position = order
            .iter()
            .position(|&i| i == anchor)
            .ok_or("The target file no longer exists.")?
            + 1;
        order.insert(position, image.files.len());
        let mut files = image.files.clone();
        files.push(file.clone());
        return ripple(image, &files, &order, position);
    }
    let blocks = if is_game {
        if image.files.iter().any(|f| f.kind == "GAME")
            || (0..needed).any(|b| fat_entry(&image.bytes, b) != FAT_FREE)
        {
            return Err("A VMU game requires consecutive free blocks starting at block 0, and only one game can be installed.".into());
        }
        (0..needed).collect()
    } else {
        free_run(&image.bytes, image.capacity, needed).ok_or_else(|| format!("No contiguous space is large enough for {} ({needed} blocks). Right-click a save and Paste File to make room using ripple placement.", file.name))?
    };
    let mut bytes = image.bytes.clone();
    let entry = write_file(&mut bytes, file, &blocks);
    let mut entries: Vec<_> = image.files.iter().map(|f| f.directory_entry).collect();
    entries.push(entry);
    finish(image, bytes, entries)
}

pub fn can_move(image: &VmuImage, index: usize, higher: bool) -> bool {
    let order = data_order(image);
    order.iter().position(|&i| i == index).is_some_and(|p| {
        if higher {
            p > 0
        } else {
            p + 1 < order.len()
        }
    })
}

pub fn move_file(image: &VmuImage, index: usize, higher: bool) -> Result<VmuImage, String> {
    let mut order = data_order(image);
    let position = order
        .iter()
        .position(|&i| i == index)
        .ok_or("A VMU game must stay at block 0.")?;
    if !can_move(image, index, higher) {
        return Err("This save is already at the end of the block order.".into());
    }
    let other = if higher { position - 1 } else { position + 1 };
    order.swap(position, other);
    ripple(image, &image.files, &order, position.min(other))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(name: &str, blocks: u16, game: bool) -> VmuFile {
        let mut entry = [0; 32];
        entry[0] = if game { 0xcc } else { 0x33 };
        entry[4..4 + name.len()].copy_from_slice(name.as_bytes());
        entry[24..26].copy_from_slice(&blocks.to_le_bytes());
        entry[26] = u8::from(game);
        parse_file(
            &entry,
            name.into(),
            0,
            blocks,
            usize::from(game) * 512,
            vec![42; blocks as usize * 512],
        )
    }
    #[test]
    fn first_fit_uses_holes_and_ripple_moves_only_the_following_saves() {
        let base = parse_image(&super::super::tests::formatted(200)).unwrap();
        let a = paste(&base, &save("A", 4, false), Placement::FirstFit).unwrap();
        let b = paste(&a, &save("B", 5, false), Placement::FirstFit).unwrap();
        let c = paste(&b, &save("C", 3, false), Placement::FirstFit).unwrap();
        let hole = delete(&c, 1).unwrap();
        let filled = paste(&hole, &save("HOLE", 4, false), Placement::FirstFit).unwrap();
        assert_eq!(filled.files[2].first_block, 195);
        assert_eq!(filled.files[1].first_block, c.files[2].first_block);
        let inserted = paste(&c, &save("INSERT", 2, false), Placement::After(0)).unwrap();
        assert_eq!(inserted.files[0].first_block, 199);
        assert_eq!(inserted.files[3].first_block, 195);
        assert_eq!(inserted.files[1].first_block, 193);
        assert_eq!(inserted.files[2].first_block, 188);
        for n in 0..3 {
            assert_eq!(inserted.files[n].bytes, c.files[n].bytes);
        }
        let moved = move_file(&inserted, 2, true).unwrap();
        assert_eq!(moved.files[2].first_block, 193);
        assert_eq!(moved.files[1].first_block, 190);
        assert_eq!(moved.files[0].first_block, 199);
        assert_eq!(moved.files[3].first_block, 195);
        assert_eq!(moved.free, inserted.free);
        for n in 0..4 {
            assert_eq!(moved.files[n].bytes, inserted.files[n].bytes);
        }
        assert_eq!(moved.original_bytes, inserted.original_bytes);
    }
    #[test]
    fn cross_format_copy_preserves_payload_metadata_order_and_original() {
        let a = parse_image(&super::super::tests::formatted(241)).unwrap();
        let a = paste(&a, &save("FIRST", 3, false), Placement::FirstFit).unwrap();
        let a = paste(&a, &save("SECOND", 2, false), Placement::FirstFit).unwrap();
        assert_eq!(a.files[1].name, "SECOND");
        let copied = copy(&a, 0).unwrap();
        let b = parse_image(&super::super::tests::formatted(200)).unwrap();
        let b = paste(&b, &copied, Placement::FirstFit).unwrap();
        assert_eq!(b.files[0].bytes, copied.bytes);
        assert_eq!(
            &b.files[0].directory_entry[4..],
            &copied.directory_entry[4..]
        );
        assert_eq!(b.free, 197);
        assert_eq!(b.files[0].first_block, 199);
        let deleted = delete(&b, 0).unwrap();
        assert_eq!(deleted.free, 200);
        assert!(deleted.files.is_empty());
        assert_eq!(deleted.original_bytes, b.original_bytes);
    }
    #[test]
    fn failed_edits_leave_image_untouched_and_games_start_at_zero() {
        let base = parse_image(&super::super::tests::formatted(200)).unwrap();
        assert!(
            paste(&base, &save("LARGE", 201, false), Placement::FirstFit)
                .unwrap_err()
                .contains("free blocks")
        );
        let game = paste(&base, &save("GAME", 2, true), Placement::FirstFit).unwrap();
        assert_eq!(game.files[0].first_block, 0);
        assert!(paste(&game, &save("GAME2", 2, true), Placement::FirstFit).is_err());
        assert!(paste(&game, &game.files[0], Placement::FirstFit).is_err());
        let mut corrupt = game.clone();
        set_fat(&mut corrupt.bytes, 0, 0);
        assert!(delete(&corrupt, 0).is_err());
        let mut shared = paste(&game, &save("DATA", 1, false), Placement::FirstFit).unwrap();
        shared.files[1].first_block = 0;
        assert!(delete(&shared, 1).is_err());
        assert_eq!(base.free, 200);
        assert!(base.original_bytes.is_none());
    }
}
