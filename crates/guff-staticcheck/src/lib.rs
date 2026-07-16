//! guff-staticcheck — Rust port of [Staticcheck](https://staticcheck.dev/) checks.
//!
//! Each check lives in its own module (mirroring `honnef.co/go/tools/staticcheck`
//! and `honnef.co/go/tools/simple`). Analyzers plug into [`guff_runner`] like any
//! other `go/analysis` pass.

mod fakejson;
mod render;
mod structtag;
mod stdlib_deprecations;
pub mod s1000;
pub mod s1001;
pub mod s1002;
pub mod s1003;
pub mod s1004;
pub mod s1005;
pub mod s1006;
pub mod s1007;
pub mod s1008;
pub mod s1009;
pub mod s1010;
pub mod s1011;
pub mod s1012;
pub mod s1016;
pub mod s1017;
pub mod s1018;
pub mod s1019;
pub mod s1020;
pub mod s1021;
pub mod s1023;
pub mod s1024;
pub mod s1025;
pub mod s1028;
pub mod s1029;
pub mod s1030;
pub mod s1031;
pub mod s1032;
pub mod s1033;
pub mod s1034;
pub mod s1035;
pub mod s1036;
pub mod s1037;
pub mod s1038;
pub mod s1039;
pub mod s1040;
pub mod st1000;
pub mod st1001;
pub mod st1006;
pub mod st1011;
pub mod st1012;
pub mod st1015;
pub mod st1017;
pub mod st1019;
pub mod sa1000;
pub mod sa1001;
pub mod sa1002;
pub mod sa1003;
pub mod sa1004;
pub mod sa1005;
pub mod sa1006;
pub mod sa1007;
pub mod sa1008;
pub mod sa1010;
pub mod sa1011;
pub mod sa1012;
pub mod sa1013;
pub mod sa1014;
pub mod sa1015;
pub mod sa1016;
pub mod sa1017;
pub mod sa1018;
pub mod sa1019;
pub mod sa1021;
pub mod sa1020;
pub mod sa1023;
pub mod sa1025;
pub mod sa1028;
pub mod sa1029;
pub mod sa1030;
pub mod sa1032;
pub mod sa1027;
pub mod sa1031;
pub mod sa1024;
pub mod sa1026;
pub mod sa2000;
pub mod sa2001;
pub mod sa2002;
pub mod sa2003;
pub mod sa3000;
pub mod sa3001;

pub mod sa4000;
pub mod sa4001;
pub mod sa4003;
pub mod sa4004;
pub mod sa4005;
pub mod sa4006;
pub mod sa4008;
pub mod sa4009;
pub mod sa4010;
pub mod sa4011;
pub mod sa4012;
pub mod sa4013;
pub mod sa4014;
pub mod sa4015;
pub mod sa4016;
pub mod sa4017;
pub mod sa4018;
pub mod sa4019;
pub mod sa4020;
pub mod sa4021;
pub mod sa4022;
pub mod sa4023;
pub mod sa4024;
pub mod sa4025;
pub mod sa4026;
pub mod sa4027;
pub mod sa4028;
pub mod sa4029;
pub mod sa4030;
pub mod sa4031;
pub mod sa4032;
pub mod sa5000;
pub mod sa5001;
pub mod sa5002;
pub mod sa5003;
pub mod sa5004;
pub mod sa5005;
pub mod sa5007;
pub mod sa5008;
pub mod sa5009;
pub mod sa5010;
pub mod sa5011;
pub mod sa5012;
pub mod sa6000;
pub mod sa6001;
pub mod sa6002;
pub mod sa6003;
pub mod sa6005;
pub mod sa6006;
pub mod sa9001;
pub mod sa9002;
pub mod sa9003;
pub mod sa9004;
pub mod sa9005;
pub mod sa9006;
pub mod sa9007;
pub mod sa9008;
pub mod sa9009;
pub mod sa9010;

/// All ported Staticcheck/simple analyzers.
pub fn analyzers() -> Vec<&'static guff_analysis::Analyzer> {
    vec![
        s1000::analyzer(),
        s1001::analyzer(),
        s1002::analyzer(),
        s1003::analyzer(),
        s1004::analyzer(),
        s1005::analyzer(),
        s1006::analyzer(),
        s1007::analyzer(),
        s1008::analyzer(),
        s1009::analyzer(),
        s1010::analyzer(),
        s1011::analyzer(),
        s1012::analyzer(),
        s1016::analyzer(),
        s1017::analyzer(),
        s1018::analyzer(),
        s1019::analyzer(),
        s1020::analyzer(),
        s1021::analyzer(),
        s1023::analyzer(),
        s1024::analyzer(),
        s1025::analyzer(),
        s1028::analyzer(),
        s1029::analyzer(),
        s1030::analyzer(),
        s1031::analyzer(),
        s1032::analyzer(),
        s1033::analyzer(),
        s1034::analyzer(),
        s1035::analyzer(),
        s1036::analyzer(),
        s1037::analyzer(),
        s1038::analyzer(),
        s1039::analyzer(),
        s1040::analyzer(),
        st1000::analyzer(),
        st1001::analyzer(),
        st1006::analyzer(),
        st1011::analyzer(),
        st1012::analyzer(),
        st1015::analyzer(),
        st1017::analyzer(),
        st1019::analyzer(),
        sa1000::analyzer(),
        sa1001::analyzer(),
        sa1002::analyzer(),
        sa1003::analyzer(),
        sa1004::analyzer(),
        sa1005::analyzer(),
        sa1006::analyzer(),
        sa1007::analyzer(),
        sa1008::analyzer(),
        sa1010::analyzer(),
        sa1011::analyzer(),
        sa1012::analyzer(),
        sa1013::analyzer(),
        sa1014::analyzer(),
        sa1015::analyzer(),
        sa1016::analyzer(),
        sa1017::analyzer(),
        sa1018::analyzer(),
        sa1019::analyzer(),
        sa1020::analyzer(),
        sa1021::analyzer(),
        sa1023::analyzer(),
        sa1024::analyzer(),
        sa1025::analyzer(),
        sa1026::analyzer(),
        sa1027::analyzer(),
        sa1028::analyzer(),
        sa1029::analyzer(),
        sa1030::analyzer(),
        sa1031::analyzer(),
        sa1032::analyzer(),
        sa2000::analyzer(),
        sa2001::analyzer(),
        sa2002::analyzer(),
        sa2003::analyzer(),
        sa3000::analyzer(),
        sa3001::analyzer(),

        sa4000::analyzer(),
        sa4001::analyzer(),
        sa4003::analyzer(),
        sa4004::analyzer(),
        sa4005::analyzer(),
        sa4006::analyzer(),
        sa4008::analyzer(),
        sa4009::analyzer(),
        sa4010::analyzer(),
        sa4011::analyzer(),
        sa4012::analyzer(),
        sa4013::analyzer(),
        sa4014::analyzer(),
        sa4015::analyzer(),
        sa4016::analyzer(),
        sa4017::analyzer(),
        sa4018::analyzer(),
        sa4019::analyzer(),
        sa4020::analyzer(),
        sa4021::analyzer(),
        sa4022::analyzer(),
        sa4023::analyzer(),
        sa4024::analyzer(),
        sa4025::analyzer(),
        sa4026::analyzer(),
        sa4027::analyzer(),
        sa4028::analyzer(),
        sa4029::analyzer(),
        sa4030::analyzer(),
        sa4031::analyzer(),
        sa4032::analyzer(),
        sa5000::analyzer(),
        sa5001::analyzer(),
        sa5002::analyzer(),
        sa5003::analyzer(),
        sa5004::analyzer(),
        sa5005::analyzer(),
        sa5007::analyzer(),
        sa5008::analyzer(),
        sa5009::analyzer(),
        sa5010::analyzer(),
        sa5011::analyzer(),
        sa5012::analyzer(),
        sa6000::analyzer(),
        sa6001::analyzer(),
        sa6002::analyzer(),
        sa6003::analyzer(),
        sa6005::analyzer(),
        sa6006::analyzer(),
        sa9001::analyzer(),
        sa9002::analyzer(),
        sa9003::analyzer(),
        sa9004::analyzer(),
        sa9005::analyzer(),
        sa9006::analyzer(),
        sa9007::analyzer(),
        sa9008::analyzer(),
        sa9009::analyzer(),
        sa9010::analyzer(),
    ]
}
