use bit_vec::BitVec;

pub trait BitVecExt {
    fn zeroes(&self) -> Vec<usize>;

    fn ones(&self) -> Vec<usize>;
}

impl BitVecExt for BitVec {
    fn zeroes(&self) -> Vec<usize> {
        self.iter()
            .enumerate()
            .filter_map(|(i, b)| if !b { Some(i) } else { None })
            .collect()
    }

    fn ones(&self) -> Vec<usize> {
        self.iter()
            .enumerate()
            .filter_map(|(i, b)| if b { Some(i) } else { None })
            .collect()
    }
}