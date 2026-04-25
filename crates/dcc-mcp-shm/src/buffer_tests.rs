//! Unit tests for the `SharedBuffer` type and orphan GC.

use super::*;

mod test_create {
    use super::*;

    #[test]
    fn test_create_and_capacity() {
        let buf = SharedBuffer::create(1024).unwrap();
        assert_eq!(buf.capacity(), 1024);
    }

    #[test]
    fn test_create_zero_capacity_fails() {
        let result = SharedBuffer::create(0);
        assert!(matches!(result, Err(ShmError::InvalidArgument(_))));
    }

    #[test]
    fn test_initial_data_len_is_zero() {
        let buf = SharedBuffer::create(512).unwrap();
        assert_eq!(buf.data_len().unwrap(), 0);
    }

    #[test]
    fn test_read_empty_returns_empty_vec() {
        let buf = SharedBuffer::create(256).unwrap();
        let data = buf.read().unwrap();
        assert!(data.is_empty());
    }
}

mod test_write_read {
    use super::*;

    #[test]
    fn test_write_and_read_roundtrip() {
        let buf = SharedBuffer::create(4096).unwrap();
        let payload = b"Hello, DCC MCP shared memory!";
        buf.write(payload).unwrap();
        let out = buf.read().unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn test_write_updates_data_len() {
        let buf = SharedBuffer::create(1024).unwrap();
        buf.write(b"abc").unwrap();
        assert_eq!(buf.data_len().unwrap(), 3);
    }

    #[test]
    fn test_write_too_large_returns_error() {
        let buf = SharedBuffer::create(4).unwrap();
        let result = buf.write(b"12345");
        assert!(matches!(result, Err(ShmError::BufferTooSmall { .. })));
    }

    #[test]
    fn test_overwrite_replaces_data() {
        let buf = SharedBuffer::create(1024).unwrap();
        buf.write(b"first").unwrap();
        buf.write(b"second").unwrap();
        let out = buf.read().unwrap();
        assert_eq!(out, b"second");
    }

    #[test]
    fn test_clear_resets_data_len() {
        let buf = SharedBuffer::create(256).unwrap();
        buf.write(b"data").unwrap();
        buf.clear().unwrap();
        assert_eq!(buf.data_len().unwrap(), 0);
        let out = buf.read().unwrap();
        assert!(out.is_empty());
    }
}

mod test_descriptor {
    use super::*;

    #[test]
    fn test_descriptor_roundtrip_json() {
        let buf = SharedBuffer::create(2048).unwrap();
        let desc = BufferDescriptor::from_buffer(&buf).unwrap();
        let json = desc.to_json().unwrap();
        let desc2 = BufferDescriptor::from_json(&json).unwrap();
        assert_eq!(desc2.capacity, 2048);
        assert_eq!(desc2.id, buf.id);
    }

    #[test]
    fn test_open_via_descriptor() {
        let buf = SharedBuffer::create(1024).unwrap();
        buf.write(b"cross-process").unwrap();
        let desc = BufferDescriptor::from_buffer(&buf).unwrap();
        let buf2 = SharedBuffer::open(&desc.name, &desc.id).unwrap();
        let out = buf2.read().unwrap();
        assert_eq!(out, b"cross-process");
    }
}

mod test_clone {
    use super::*;

    #[test]
    fn test_clone_shares_same_region() {
        let buf = SharedBuffer::create(512).unwrap();
        buf.write(b"shared").unwrap();
        let buf2 = buf.clone();
        let out = buf2.read().unwrap();
        assert_eq!(out, b"shared");
    }

    #[test]
    fn test_write_via_clone_visible_in_original() {
        let buf = SharedBuffer::create(512).unwrap();
        let buf2 = buf.clone();
        buf2.write(b"written by clone").unwrap();
        let out = buf.read().unwrap();
        assert_eq!(out, b"written by clone");
    }
}

mod test_ttl {
    use super::*;

    #[test]
    fn test_no_ttl_never_expired() {
        let buf = SharedBuffer::create(256).unwrap();
        assert!(!buf.is_expired().unwrap());
    }

    #[test]
    fn test_long_ttl_not_expired() {
        let buf = SharedBuffer::create_with_ttl("ttl-long", 256, Some(Duration::from_secs(3600)))
            .unwrap();
        assert!(!buf.is_expired().unwrap());
    }

    #[test]
    fn test_descriptor_contains_ttl() {
        let buf =
            SharedBuffer::create_with_ttl("ttl-desc", 256, Some(Duration::from_secs(30))).unwrap();
        let desc = BufferDescriptor::from_buffer(&buf).unwrap();
        assert_eq!(desc.ttl_secs, 30);
    }

    #[test]
    fn test_descriptor_zero_ttl_when_none() {
        let buf = SharedBuffer::create(256).unwrap();
        let desc = BufferDescriptor::from_buffer(&buf).unwrap();
        assert_eq!(desc.ttl_secs, 0);
    }
}

mod test_gc {
    use super::*;

    #[test]
    fn test_gc_orphans_returns_usize() {
        // Just ensure the function compiles and returns without panic.
        let _ = gc_orphans(Duration::from_secs(60));
    }
}
