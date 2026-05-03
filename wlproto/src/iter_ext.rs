use std::fmt;


#[derive(Debug)]
pub enum SingleIterError<T> {
    NoItem,
    MultipleItems { first: T, second: T },
}
impl<T> fmt::Display for SingleIterError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SingleIterError::NoItem
                => write!(f, "iterator returned no item"),
            SingleIterError::MultipleItems { .. }
                => write!(f, "iterator returned more than one item"),
        }
    }
}


pub trait SingleIterExt<T> {
    fn single(&mut self) -> Result<T, SingleIterError<T>>;
}
impl<T, I: Iterator<Item = T>> SingleIterExt<T> for I {
    fn single(&mut self) -> Result<T, SingleIterError<T>> {
        match self.next() {
            Some(first) => {
                match self.next() {
                    Some(second) => Err(SingleIterError::MultipleItems {
                        first,
                        second,
                    }),
                    None => Ok(first),
                }
            },
            None => Err(SingleIterError::NoItem),
        }
    }
}
