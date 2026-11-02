//! A file that crashes, hangs or floods web-ifc must only end its worker
//! process: `read` returns an error, and the next read works.

use std::time::Duration;

use falcon_ifc::{Error, Limits, read};

/// A file around `body`: a project in metres with a 3D context #2 and origin
/// placement #4.
fn file(body: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [ReferenceView]'),'2;1');\n\
         FILE_NAME('t.ifc','2026-11-01T00:00:00',(''),(''),'falcon','falcon','');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n\
         #1=IFCPROJECT('0001',$,'P',$,$,$,$,(#2),#3);\n#2=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#4,$);\n\
         #3=IFCUNITASSIGNMENT((#5));\n#4=IFCAXIS2PLACEMENT3D(#6,$,$);\n#5=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n\
         #6=IFCCARTESIANPOINT((0.,0.,0.));\n{body}ENDSEC;\nEND-ISO-10303-21;\n"
    )
}

/// One product `#103` with the item `#31` (defined in `items`).
fn product(items: &str) -> String {
    file(&format!(
        "#20=IFCCARTESIANPOINT((0.,0.));\n#21=IFCAXIS2PLACEMENT2D(#20,$);\n#22=IFCDIRECTION((0.,0.,1.));\n{items}\
         #100=IFCSHAPEREPRESENTATION(#2,'Body','SweptSolid',(#31));\n#101=IFCPRODUCTDEFINITIONSHAPE($,$,(#100));\n\
         #102=IFCLOCALPLACEMENT($,#4);\n#103=IFCWALL('0002',$,'Thing',$,$,#102,#101,$,$);\n"
    ))
}

fn good() -> String {
    product("#30=IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,4.,0.2);\n#31=IFCEXTRUDEDAREASOLID(#30,#4,#22,3.);\n")
}

/// A mapped item that contains itself: web-ifc recurses until its stack ends.
fn crashing() -> String {
    product(
        "#32=IFCSHAPEREPRESENTATION(#2,'Body','MappedRepresentation',(#31));\n#33=IFCREPRESENTATIONMAP(#4,#32);\n\
         #35=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#6,1.,$);\n#31=IFCMAPPEDITEM(#33,#35);\n",
    )
}

fn check_good() {
    let scene = read(good().as_bytes(), Limits::default()).unwrap();
    assert_eq!(scene.triangle_count(), 12);
    assert_eq!(scene.products.iter().filter(|p| p.geometries > 0).count(), 1);
}

#[test]
fn a_good_file_comes_back_whole() {
    if falcon_ifc::available() {
        check_good();
    }
}

#[test]
fn a_crash_ends_only_the_worker() {
    if !falcon_ifc::available() {
        return;
    }
    assert_eq!(read(crashing().as_bytes(), Limits::default()), Err(Error::Crashed));
    check_good();
}

/// A conversion-based length unit whose measure is missing: web-ifc spins
/// with memory climbing.
fn hanging() -> String {
    good().replace("#3=IFCUNITASSIGNMENT((#5));", "#3=IFCUNITASSIGNMENT((#7));\n#7=IFCCONVERSIONBASEDUNIT(#8,.LENGTHUNIT.,'inch',#999);")
}

#[test]
fn a_timeout_ends_the_worker() {
    if !falcon_ifc::available() {
        return;
    }
    let limits = Limits { timeout: Duration::from_secs(1), ..Limits::default() };
    let start = std::time::Instant::now();
    assert_eq!(read(hanging().as_bytes(), limits), Err(Error::TimedOut));
    assert!(start.elapsed() < Duration::from_secs(10));
    check_good();
}

#[test]
fn a_memory_limit_ends_the_worker() {
    if !falcon_ifc::available() {
        return;
    }
    let limits = Limits { max_memory: 1 << 10, ..Limits::default() };
    assert_eq!(read(hanging().as_bytes(), limits), Err(Error::OutOfMemory));
    check_good();
}

#[test]
fn garbage_is_an_error_or_an_empty_scene() {
    if !falcon_ifc::available() {
        return;
    }
    for bytes in [&b""[..], b"\0\xff\xfe garbage \x01\x02", b"ISO-10303-21; DATA; #1=(;", &[0xAB; 4096]] {
        if let Ok(scene) = read(bytes, Limits::default()) {
            assert_eq!(scene.triangle_count(), 0);
        }
    }
}

/// Reads damaged copies of the file: dropped lines, changed references and
/// flipped characters. Whatever comes back must be well formed.
fn fuzz(source: &[u8], rounds: usize, seed: u64) {
    let limits = Limits { timeout: Duration::from_secs(5), max_memory: 4 << 30, ..Limits::default() };
    let lines: Vec<&[u8]> = source.split(|&c| c == b'\n').collect();
    let mut state = seed;
    let mut next = move |below: usize| {
        // SplitMix64, so a failing round can be reproduced from its seed.
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % below as u64) as usize
    };
    for _ in 0..rounds {
        let mode = next(3);
        let mut damaged = Vec::with_capacity(source.len());
        for line in &lines {
            let mut line = line.to_vec();
            match mode {
                0 if next(20) == 0 => continue,
                1 if next(10) == 0 => {
                    if let Some(i) = line.iter().skip(1).position(|&c| c == b'#')
                        && i + 2 < line.len()
                    {
                        line[i + 2] = b'1' + next(9) as u8;
                    }
                }
                2 if next(15) == 0 && !line.is_empty() => {
                    let i = next(line.len());
                    line[i] = b"0123456789.,()#$-E'*"[next(20)];
                }
                _ => {}
            }
            damaged.extend_from_slice(&line);
            damaged.push(b'\n');
        }
        if let Ok(scene) = read(&damaged, limits) {
            for g in &scene.geometries {
                assert!(g.indices.iter().all(|&i| (i as usize) < g.positions.len()));
            }
        }
    }
}

#[test]
fn damaged_files_never_reach_the_caller() {
    if falcon_ifc::available() {
        fuzz(good().as_bytes(), 40, 1);
    }
}

/// Fuzzes every `.ifc` under `$FALCON_IFC_FUZZ_DIR` of at most 400 kB,
/// `$FALCON_IFC_FUZZ_ROUNDS` rounds each (default 5). Without the variable
/// it does nothing.
#[test]
fn damaged_sample_files() {
    let Some(dir) = std::env::var_os("FALCON_IFC_FUZZ_DIR") else { return };
    if !falcon_ifc::available() {
        return;
    }
    let rounds = std::env::var("FALCON_IFC_FUZZ_ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut stack = vec![std::path::PathBuf::from(dir)];
    let mut files = 0;
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ifc")) && e.metadata().is_ok_and(|m| m.len() <= 400_000) {
                fuzz(&std::fs::read(&p).unwrap(), rounds, files);
                files += 1;
            }
        }
    }
    println!("{files} files, {rounds} rounds each");
}
