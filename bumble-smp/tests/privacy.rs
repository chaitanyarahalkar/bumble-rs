use bumble::{Address, AddressType};
use bumble_smp::{resolvable_private_address, verify_resolvable_private_address, AddressResolver};

fn address(value: &str, address_type: AddressType) -> Address {
    Address::parse(value, address_type).unwrap()
}

#[test]
fn address_resolver_matches_the_upstream_ah_vector() {
    let irk = [
        0x9B, 0x7D, 0x39, 0x0A, 0xA6, 0x10, 0x10, 0x34, 0x05, 0xAD, 0xC8, 0x57, 0xA3, 0x34, 0x02,
        0xEC,
    ];
    let identity = address("C4:F2:17:1A:1D:BB", AddressType::PUBLIC_DEVICE);
    // reversed_hex('708194') from tests/smp_test.py, with the RPA marker
    // already present in its most-significant octet.
    let rpa = resolvable_private_address(&irk, [0x94, 0x81, 0x70]);
    assert!(rpa.is_resolvable());
    assert_eq!(&rpa.address_bytes()[..3], &[0xAA, 0xFB, 0x0D]);
    assert!(verify_resolvable_private_address(&irk, &rpa));
    assert!(!verify_resolvable_private_address(&[0; 16], &rpa));

    let resolver = AddressResolver::new(vec![(irk.to_vec(), identity.clone())]);
    assert!(resolver.can_resolve_to(&identity));
    let resolved = resolver.resolve(&rpa).unwrap();
    assert_eq!(resolved.address_bytes(), identity.address_bytes());
    assert_eq!(resolved.address_type(), AddressType::PUBLIC_IDENTITY);
}

#[test]
fn wrong_irk_non_rpa_and_invalid_key_do_not_resolve() {
    let identity = address("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE);
    let rpa = resolvable_private_address(&[1; 16], [2, 3, 4]);
    let resolver = AddressResolver::new(vec![
        (vec![9; 16], identity.clone()),
        (vec![7; 15], identity),
    ]);
    assert_eq!(resolver.resolve(&rpa), None);
    assert_eq!(
        resolver.resolve(&address("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE)),
        None
    );
    assert!(!verify_resolvable_private_address(
        &[9; 16],
        &address("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE)
    ));
}
