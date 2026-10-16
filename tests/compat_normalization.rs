#[path = "common/compat.rs"]
#[allow(dead_code, reason = "only output normalization is exercised here")]
mod compat;

#[test]
fn compatibility_decodes_only_the_drive_colon_in_annotation_file_properties() {
    let encoded = "::group::C:\\work\\a.yaml\r\n\
                   ::error file=C%3A\\work\\a.yaml,line=1,col=2::message C%3A %25\r\n\
                   ::warning file=D%3A/work/b.yaml,line=3,col=4::warning\r\n\
                   ::error file=/work/C%3A.yaml,line=1,col=2::message\r\n\
                   ordinary file=C%3A\\work\\a.yaml\r\n";
    let expected = "::group::C:\\work\\a.yaml\n\
                    ::error file=C:\\work\\a.yaml,line=1,col=2::message C%3A %25\n\
                    ::warning file=D:/work/b.yaml,line=3,col=4::warning\n\
                    ::error file=/work/C%3A.yaml,line=1,col=2::message\n\
                    ordinary file=C%3A\\work\\a.yaml\n";
    assert_eq!(
        compat::normalize_output(encoded.into(), String::new()),
        expected
    );
}
