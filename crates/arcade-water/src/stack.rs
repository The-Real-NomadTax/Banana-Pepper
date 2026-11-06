//! Linux x86-64 initial stack construction (argc/argv/envp/auxv).
pub struct InitialStack {
    pub address: u64,
    pub bytes: Vec<u8>,
}

pub fn build(top: u64, argv: &[String], env: &[String], mut auxv: Vec<(u64,u64)>, random: [u8;16]) -> Result<InitialStack,String> {
    if argv.is_empty() { return Err("Guest argv must include argv[0]".into()); }
    let mut strings = Vec::new();
    let mut arguments = Vec::new();
    let mut environment = Vec::new();
    // Build strings in ascending address order, with pointer offsets fixed up
    // once the size of the vector portion is known.
    for (items, pointers) in [(argv, &mut arguments), (env, &mut environment)] {
        for value in items {
            if value.as_bytes().contains(&0) { return Err("Guest strings cannot contain NUL".into()); }
            pointers.push(strings.len());
            strings.extend_from_slice(value.as_bytes()); strings.push(0);
        }
    }
    let random_offset = strings.len(); strings.extend_from_slice(&random);
    if strings.len() > 64 * 1024 { return Err("Guest argument/environment strings exceed 64 KiB".into()); }
    auxv.retain(|(key,_)| !matches!(*key,0|25|31));
    let word_count = 1 + argv.len() + 1 + env.len() + 1 + (auxv.len()+3)*2;
    let total = word_count.checked_mul(8).and_then(|n| n.checked_add(strings.len())).ok_or("Stack size overflow")?;
    let address = top.checked_sub(total as u64).ok_or("Stack address underflow")? & !15;
    let strings_base = address + word_count as u64 * 8;
    let mut words = vec![argv.len() as u64];
    words.extend(arguments.iter().map(|p| strings_base + *p as u64)); words.push(0);
    words.extend(environment.iter().map(|p| strings_base + *p as u64)); words.push(0);
    auxv.push((25,strings_base + random_offset as u64));
    auxv.push((31,strings_base + arguments[0] as u64));
    auxv.push((0,0));
    for (kind,value) in auxv { words.extend([kind,value]); }
    let mut bytes: Vec<u8> = words.into_iter().flat_map(u64::to_le_bytes).collect();
    bytes.extend(strings);
    Ok(InitialStack { address, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_stack_has_valid_pointers_alignment_and_auxv() {
        let stack=build(0x800000,&["/bin/test".into(),"argument with spaces".into()],&["LANG=C".into()],vec![(6,4096)],[7;16]).unwrap();
        assert_eq!(stack.address%16,0);
        let word=|i:usize|u64::from_le_bytes(stack.bytes[i*8..i*8+8].try_into().unwrap());
        assert_eq!(word(0),2);assert_eq!(word(3),0);assert_eq!(word(5),0);
        for (i,expected) in [(1,b"/bin/test\0".as_slice()),(2,b"argument with spaces\0".as_slice()),(4,b"LANG=C\0".as_slice())] {
            let offset=(word(i)-stack.address) as usize;
            assert_eq!(&stack.bytes[offset..offset+expected.len()],expected);
        }
        assert_eq!((word(6),word(7)),(6,4096));
        assert_eq!(word(8),25);
        let random=(word(9)-stack.address) as usize;
        assert_eq!(&stack.bytes[random..random+16],&[7;16]);
        assert_eq!((word(12),word(13)),(0,0));
    }
}
