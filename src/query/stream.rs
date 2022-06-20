use fallible_iterator::FallibleIterator;
use std::rc::Rc;

#[derive(Clone)]
pub struct Stream<'a, T, E>(Rc<dyn CloneableStream<'a, T, E> + 'a>);

impl<'a, T: 'a, E> Stream<'a, T, E>
{
    pub fn new<I>(iter: I) -> Stream<'a, I::Item, I::Error>
        where I: 'a + FallibleIterator<Item=T, Error=E> + Clone
    {
        Stream(Rc::new(iter))
    }
}

trait CloneableStream<'a, T, E> {
    fn rc_clone(&self) -> Rc<dyn CloneableStream<'a, T, E> + 'a>;
    fn cs_next(&mut self) -> Result<Option<T>, E>;
}

impl<'a, I> CloneableStream<'a, I::Item, I::Error> for I
where
    I: 'a + FallibleIterator + Clone,
    I::Item: 'a
{
    fn rc_clone(&self) -> Rc<dyn CloneableStream<'a, I::Item, I::Error> + 'a> {
        Rc::new(self.clone())
    }

    fn cs_next(&mut self) -> Result<Option<I::Item>, I::Error> {
        self.next()
    }
}

impl<'a, T, E> FallibleIterator for Stream<'a, T, E> {
    type Item = T;
    type Error = E;
    fn next(&mut self) -> Result<Option<T>, E> {
        // If we're sharing the underlying iterator tree with anyone, we need
        // exclusive access to it before we draw values from it, since `next`
        // has side effects.
        if Rc::strong_count(&self.0) > 1 {
            self.0 = self.0.rc_clone();
        }

        // We ensured that we are the sole owner of `self.0`, so this unwrap
        // should always succeed.
        Rc::get_mut(&mut self.0).unwrap().cs_next()
    }
}
