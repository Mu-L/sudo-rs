use super::*;
use crate::ast;
use basic_parser::{parse_eval, parse_lines, parse_string};
use std::iter;

#[derive(PartialEq)]
struct Named(&'static str);

fn dummy_cksum(name: &str) -> u32 {
    if name == "root" {
        0
    } else {
        1000 + name.chars().fold(0, |x, y| (x * 97 + y as u32) % 1361)
    }
}

impl UnixUser for Named {
    fn has_name(&self, name: &str) -> bool {
        self.0 == name
    }

    fn in_group_by_name(&self, name: &str) -> bool {
        self.has_name(name)
    }

    fn in_group_by_gid(&self, gid: u32) -> bool {
        dummy_cksum(self.0) == gid
    }

    fn is_root(&self) -> bool {
        self.0 == "root"
    }
}

impl UnixGroup for Named {
    fn as_gid(&self) -> sudo_system::interface::GroupId {
        dummy_cksum(&self.0)
    }
    fn try_as_name(&self) -> Option<&str> {
        Some(&self.0)
    }
}

macro_rules! request {
    ($user:ident, $group:ident) => {
        (&Named(stringify!($user)), &Named(stringify!($group)))
    };
}

macro_rules! sudoer {
    ($h:expr $(,$e:expr)*) => {
	    parse_lines(&mut
		(
		    iter::once($h)
		    $(
			.chain(iter::once($e))
		    )*
		)
		.map(|s|s.chars().chain(iter::once('\n')))
		.flatten()
		.peekable()
	    )
	    .into_iter()
	    .map(|x| Ok::<_,basic_parser::Status>(x.unwrap()))
    }
}

// alternative to parse_eval, but goes through sudoer! directly
fn parse_line(s: &str) -> Sudo {
    sudoer![s].next().unwrap().unwrap()
}

#[test]
fn ambiguous_spec() {
    let Sudo::Spec(_) = parse_eval::<ast::Sudo>("marc, User_Alias ALL = ALL") else { todo!() };
}

#[test]
fn digest_spec() {
    let CommandSpec(_, _, digest) = parse_eval(
        "NOPASSWD: sha224: c12053ca894181bc137b940b06b2e2459e9aa7b46d2d317777f34236 /bin/ls",
    );
    let Sha2(vec) = digest;
    assert_eq!(
        *vec,
        [
            0xc1, 0x20, 0x53, 0xca, 0x89, 0x41, 0x81, 0xbc, 0x13, 0x7b, 0x94, 0x0b, 0x06, 0xb2,
            0xe2, 0x45, 0x9e, 0x9a, 0xa7, 0xb4, 0x6d, 0x2d, 0x31, 0x77, 0x77, 0xf3, 0x42, 0x36,
        ]
    )
}

#[test]
#[should_panic]
fn digest_spec_fail1() {
    // the hash length is incorrect
    parse_eval::<CommandSpec>(
        "NOPASSWD: sha224: c12053ca894181bc137b940b06b2e2459e9aa7b46d2d317777f342 /bin/ls",
    );
}

#[test]
#[should_panic]
fn digest_spec_fail2() {
    // the hash length has an odd length
    parse_eval::<CommandSpec>(
        "NOPASSWD: sha224: c12053ca894181bc137b940b06b2e2459e9aa7b46d2d317777f3421 /bin/ls",
    );
}

#[test]
#[should_panic]
fn digest_spec_fail3() {
    // the hash length has an invalid char
    parse_eval::<CommandSpec>(
        "NOPASSWD: sha224: c12053ca894181bc137b940b06b2e2459e9aa7b46d2d317777g34236 /bin/ls",
    );
}

#[test]
fn permission_test() {
    let root = || (&Named("root"), &Named("root"));

    macro_rules! FAIL {
        ([$($sudo:expr),*], $user:expr => $req:expr, $server:expr; $command:expr) => {
            let (Sudoers { rules,aliases,settings }, _) = analyze(sudoer![$($sudo),*]);
            let cmdvec = $command.split_whitespace().collect::<Vec<_>>();
            let req = Request { user: $req.0, group: $req.1, command: cmdvec[0].as_ref(), arguments: &cmdvec[1..].join(" ") };
            assert_eq!(check_permission(&Sudoers { rules, aliases, settings }, &Named($user), $server, req), None);
        }
    }

    macro_rules! pass {
        ([$($sudo:expr),*], $user:expr => $req:expr, $server:expr; $command:expr $(=> [$($list:expr),*])?) => {
            let (Sudoers { rules,aliases,settings }, _) = analyze(sudoer![$($sudo),*]);
            let cmdvec = $command.split_whitespace().collect::<Vec<_>>();
            let req = Request { user: $req.0, group: $req.1, command: &cmdvec[0].as_ref(), arguments: &cmdvec[1..].join(" ") };
            let result = check_permission(&Sudoers { rules, aliases, settings }, &Named($user), $server, req);
            $(assert_eq!(result, Some(vec![$($list),*]));)?
            assert!(!result.is_none());
        }
    }
    macro_rules! SYNTAX {
        ([$sudo:expr]) => {
            assert!(parse_string::<Sudo>($sudo).is_err())
        };
    }

    use crate::ast::Tag::*;

    SYNTAX!(["ALL ALL = (;) ALL"]);
    FAIL!(["user ALL=(ALL:ALL) ALL"], "nobody"    => root(), "server"; "/bin/hello");
    pass!(["user ALL=(ALL:ALL) ALL"], "user"      => root(), "server"; "/bin/hello");
    pass!(["user ALL=(ALL:ALL) /bin/foo"], "user" => root(), "server"; "/bin/foo");
    FAIL!(["user ALL=(ALL:ALL) /bin/foo"], "user" => root(), "server"; "/bin/hello");
    pass!(["user ALL=(ALL:ALL) /bin/foo, NOPASSWD: /bin/bar"], "user" => root(), "server"; "/bin/foo");
    pass!(["user ALL=(ALL:ALL) /bin/foo, NOPASSWD: /bin/bar"], "user" => root(), "server"; "/bin/bar" => [NoPasswd]);

    pass!(["user ALL=/bin/e##o"], "user" => root(), "vm"; "/bin/e");
    SYNTAX!(["ALL ALL=(ALL) /bin/\n/echo"]);

    pass!(["user server=(ALL:ALL) ALL"], "user" => root(), "server"; "/bin/hello");
    FAIL!(["user laptop=(ALL:ALL) ALL"], "user" => root(), "server"; "/bin/hello");

    pass!(["user ALL=!/bin/hello", "user ALL=/bin/hello"], "user" => root(), "server"; "/bin/hello");
    FAIL!(["user ALL=/bin/hello", "user ALL=!/bin/hello"], "user" => root(), "server"; "/bin/hello");

    for alias in [
        "User_Alias GROUP=user1, user2",
        "User_Alias GROUP=ALL,!user3",
    ] {
        pass!([alias,"GROUP ALL=/bin/hello"], "user1" => root(), "server"; "/bin/hello");
        pass!([alias,"GROUP ALL=/bin/hello"], "user2" => root(), "server"; "/bin/hello");
        FAIL!([alias,"GROUP ALL=/bin/hello"], "user3" => root(), "server"; "/bin/hello");
    }
    pass!(["user ALL=/bin/hello arg"], "user" => root(), "server"; "/bin/hello arg");
    pass!(["user ALL=/bin/hello  arg"], "user" => root(), "server"; "/bin/hello arg");
    pass!(["user ALL=/bin/hello arg"], "user" => root(), "server"; "/bin/hello  arg");
    FAIL!(["user ALL=/bin/hello arg"], "user" => root(), "server"; "/bin/hello boo");
    pass!(["user ALL=/bin/hello a*g"], "user" => root(), "server"; "/bin/hello  aaaarg");
    FAIL!(["user ALL=/bin/hello a*g"], "user" => root(), "server"; "/bin/hello boo");
    pass!(["user ALL=/bin/hello"], "user" => root(), "server"; "/bin/hello boo");
    FAIL!(["user ALL=/bin/hello \"\""], "user" => root(), "server"; "/bin/hello boo");
    pass!(["user ALL=/bin/hello \"\""], "user" => root(), "server"; "/bin/hello");
    pass!(["user ALL=/bin/hel*"], "user" => root(), "server"; "/bin/hello");
    pass!(["user ALL=/bin/hel*"], "user" => root(), "server"; "/bin/help");
    pass!(["user ALL=/bin/hel*"], "user" => root(), "server"; "/bin/help me");
    pass!(["user ALL=/bin/hel* *"], "user" => root(), "server"; "/bin/help");
    FAIL!(["user ALL=/bin/hel* me"], "user" => root(), "server"; "/bin/help");
    pass!(["user ALL=/bin/hel* me"], "user" => root(), "server"; "/bin/help me");
    FAIL!(["user ALL=/bin/hel* me"], "user" => root(), "server"; "/bin/help me please");

    SYNTAX!(["User_Alias, marc ALL = ALL"]);

    pass!(["User_Alias FULLTIME=ALL,!marc","FULLTIME ALL=ALL"], "user" => root(), "server"; "/bin/bash");
    FAIL!(["User_Alias FULLTIME=ALL,!marc","FULLTIME ALL=ALL"], "marc" => root(), "server"; "/bin/bash");
    FAIL!(["User_Alias FULLTIME=ALL,!marc","ALL,!FULLTIME ALL=ALL"], "user" => root(), "server"; "/bin/bash");
    pass!(["User_Alias FULLTIME=ALL,!marc","ALL,!FULLTIME ALL=ALL"], "marc" => root(), "server"; "/bin/bash");
    pass!(["Host_Alias MACHINE=laptop,server","user MACHINE=ALL"], "user" => root(), "server"; "/bin/bash");
    pass!(["Host_Alias MACHINE=laptop,server","user MACHINE=ALL"], "user" => root(), "laptop"; "/bin/bash");
    FAIL!(["Host_Alias MACHINE=laptop,server","user MACHINE=ALL"], "user" => root(), "desktop"; "/bin/bash");
    pass!(["Cmnd_Alias WHAT=/bin/dd, /bin/rm","user ALL=WHAT"], "user" => root(), "server"; "/bin/rm");
    pass!(["Cmd_Alias WHAT=/bin/dd,/bin/rm","user ALL=WHAT"], "user" => root(), "laptop"; "/bin/dd");
    FAIL!(["Cmnd_Alias WHAT=/bin/dd,/bin/rm","user ALL=WHAT"], "user" => root(), "desktop"; "/bin/bash");

    pass!(["User_Alias A=B","User_Alias B=user","A ALL=ALL"], "user" => root(), "vm"; "/bin/ls");
    pass!(["Host_Alias A=B","Host_Alias B=vm","ALL A=ALL"], "user" => root(), "vm"; "/bin/ls");
    pass!(["Cmnd_Alias A=B","Cmnd_Alias B=/bin/ls","ALL ALL=A"], "user" => root(), "vm"; "/bin/ls");

    FAIL!(["Runas_Alias TIME=%wheel,sudo","user ALL=() ALL"], "user" => request!{ sudo, sudo }, "vm"; "/bin/ls");
    pass!(["Runas_Alias TIME=%wheel,sudo","user ALL=(TIME) ALL"], "user" => request! { sudo, sudo }, "vm"; "/bin/ls");
    FAIL!(["Runas_Alias TIME=%wheel,sudo","user ALL=(:TIME) ALL"], "user" => request! { sudo, sudo }, "vm"; "/bin/ls");
    pass!(["Runas_Alias TIME=%wheel,sudo","user ALL=(:TIME) ALL"], "user" => request! { user, sudo }, "vm"; "/bin/ls");
    pass!(["Runas_Alias TIME=%wheel,sudo","user ALL=(TIME) ALL"], "user" => request! { wheel, wheel }, "vm"; "/bin/ls");

    pass!(["Runas_Alias \\"," TIME=%wheel\\",",sudo # hallo","user ALL\\","=(TIME) ALL"], "user" => request! { wheel, wheel }, "vm"; "/bin/ls");
}

#[test]
#[should_panic]
fn invalid_directive() {
    parse_eval::<ast::Sudo>("User_Alias, user Alias = user1, user2");
}

use std::ops::Neg;
use Qualified::*;
impl<T> Neg for Qualified<T> {
    type Output = Qualified<T>;
    fn neg(self) -> Qualified<T> {
        match self {
            Allow(x) => Forbid(x),
            Forbid(x) => Allow(x),
        }
    }
}

#[test]
fn directive_test() {
    let _everybody = parse_eval::<Spec<UserSpecifier>>("ALL");
    let _nobody = parse_eval::<Spec<UserSpecifier>>("!ALL");
    let y = |name| parse_eval::<Spec<UserSpecifier>>(name);
    let _not = |name| -parse_eval::<Spec<UserSpecifier>>(name);
    match parse_eval::<ast::Sudo>("User_Alias HENK = user1, user2") {
        Sudo::Decl(Directive::UserAlias(Def(name, list))) => {
            assert_eq!(name, "HENK");
            assert_eq!(list, vec![y("user1"), y("user2")]);
        }
        _ => panic!("incorrectly parsed"),
    }
}

#[test]
// the overloading of '#' causes a lot of issues
fn hashsign_test() {
    let Sudo::Spec(_) = parse_line("#42 ALL=ALL") else { panic!() };
    let Sudo::Spec(_) = parse_line("ALL ALL=(#42) ALL") else { panic!() };
    let Sudo::Spec(_) = parse_line("ALL ALL=(%#42) ALL") else { panic!() };
    let Sudo::Spec(_) = parse_line("ALL ALL=(:#42) ALL") else { panic!() };
    let Sudo::Decl(_) = parse_line("User_Alias FOO=#42, %#0, #3") else { panic!() };
    let Sudo::LineComment = parse_line("") else { panic!() };
    let Sudo::LineComment = parse_line("#this is a comment") else { panic!() };
    let Sudo::Include(_) = parse_line("#include foo") else { panic!() };
    let Sudo::IncludeDir(_) = parse_line("#includedir foo") else { panic!() };
    let Sudo::Include(x) = parse_line("#include \"foo bar\"") else { panic!() };
    assert_eq!(x, "foo bar");
    // this is fine
    let Sudo::LineComment = parse_line("#inlcudedir foo") else { panic!() };
    let Sudo::Include(_) = parse_line("@include foo") else { panic!() };
    let Sudo::IncludeDir(_) = parse_line("@includedir foo") else { panic!() };
    let Sudo::Include(x) = parse_line("@include \"foo bar\"") else { panic!() };
    assert_eq!(x, "foo bar");
}

#[test]
#[should_panic]
fn hashsign_error() {
    let Sudo::Include(_) = parse_line("#include foo bar") else { todo!() };
}

#[test]
#[should_panic]
fn include_regression() {
    let Sudo::Include(_) = parse_line("#4,#include foo") else { todo!() };
}

fn test_topo_sort(n: usize) {
    let alias = |s: &str| Qualified::Allow(Meta::<UserSpecifier>::Alias(s.to_string()));
    let stop = || Qualified::Allow(Meta::<UserSpecifier>::All);
    type Elem = Spec<UserSpecifier>;
    let test_case = |x1: Elem, x2: Elem, x3: Elem| {
        let table = vec![
            Def("AAP".to_string(), vec![x1]),
            Def("NOOT".to_string(), vec![x2]),
            Def("MIES".to_string(), vec![x3]),
        ];
        let mut err = vec![];
        let order = sanitize_alias_table(&table, &mut err);
        assert!(err.is_empty());
        let mut seen = HashSet::new();
        for Def(id, defns) in order.iter().map(|&i| &table[i]) {
            if defns.iter().any(|spec| {
                let Qualified::Allow(Meta::Alias(id2)) = spec else { return false };
                !seen.contains(id2)
            }) {
                panic!("forward reference encountered after sorting");
            }
            seen.insert(id);
        }
    };
    match n {
        0 => test_case(alias("AAP"), alias("NOOT"), stop()),
        1 => test_case(alias("AAP"), stop(), alias("NOOT")),
        2 => test_case(alias("NOOT"), alias("AAP"), stop()),
        3 => test_case(alias("NOOT"), stop(), alias("AAP")),
        4 => test_case(stop(), alias("AAP"), alias("NOOT")),
        5 => test_case(stop(), alias("NOOT"), alias("AAP")),
        _ => panic!("error in test case"),
    }
}

#[test]
fn test_topo_positive() {
    test_topo_sort(3);
    test_topo_sort(4);
}

#[test]
#[should_panic]
fn test_topo_fail0() {
    test_topo_sort(0);
}
#[test]
#[should_panic]
fn test_topo_fail1() {
    test_topo_sort(1);
}
#[test]
#[should_panic]
fn test_topo_fail2() {
    test_topo_sort(2);
}
#[test]
#[should_panic]
fn test_topo_fail5() {
    test_topo_sort(5);
}

fn fuzz_topo_sort(siz: usize) {
    for mut n in 0..(1..siz).reduce(|x, y| x * y).unwrap() {
        let name = |s: u8| std::str::from_utf8(&[65 + s]).unwrap().to_string();
        let alias = |s: String| Qualified::Allow(Meta::<UserSpecifier>::Alias(s));
        let stop = || Qualified::Allow(Meta::<UserSpecifier>::All);

        let mut data = (0..siz - 1)
            .map(|i| alias(name(i as u8)))
            .collect::<Vec<_>>();
        data.push(stop());

        for i in (1..=siz).rev() {
            let pos = n % i;
            n = n / i;
            data.swap(i - 1, pos);
        }

        let table = data
            .into_iter()
            .enumerate()
            .map(|(i, x)| Def(name(i as u8), vec![x]))
            .collect();

        let mut err = vec![];
        let order = sanitize_alias_table(&table, &mut err);
        if !err.is_empty() {
            return;
        }

        let mut seen = HashSet::new();
        for Def(id, defns) in order.iter().map(|&i| &table[i]) {
            if defns.iter().any(|spec| {
                let Qualified::Allow(Meta::Alias(id2)) = spec else { return false };
                !seen.contains(id2)
            }) {
                panic!("forward reference encountered after sorting");
            }
            seen.insert(id);
        }
        assert!(seen.len() == siz);
    }
}

#[test]
fn fuzz_topo_sort7() {
    fuzz_topo_sort(7)
}