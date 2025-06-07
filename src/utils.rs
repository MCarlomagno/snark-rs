pub fn log2_floor(v: u32) -> u32 {
    31 - v.leading_zeros()
}

mod tests {
    use super::*;

    #[test]
    fn test_log2_floor() {
        // Powers of 2
        assert_eq!(log2_floor(1), 0);
        assert_eq!(log2_floor(2), 1);
        assert_eq!(log2_floor(4), 2);
        assert_eq!(log2_floor(8), 3);
        assert_eq!(log2_floor(16), 4);
        assert_eq!(log2_floor(1024), 10);
        assert_eq!(log2_floor(1 << 31), 31);

        // Non powers of 2
        assert_eq!(log2_floor(3), 1); // 2^1 = 2 < 3 < 4 = 2^2
        assert_eq!(log2_floor(5), 2);
        assert_eq!(log2_floor(15), 3);
        assert_eq!(log2_floor(17), 4);
        assert_eq!(log2_floor(999), 9); // 2^9 = 512 < 999 < 1024 = 2^10

        // Edge case
        assert_eq!(log2_floor(u32::MAX), 31);
    }
}
