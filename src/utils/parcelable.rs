use std::collections::HashMap;

pub enum ParcelType<'a, F> {
    Function(&'a mut F),
    Parcelable(&'a mut dyn Parcelable<F>),
    Other(()),
}

#[derive(thiserror::Error, Debug)]
pub enum ParcelableError {
    #[error("error to get field: {0:?}")]
    FieldError(String),
    #[error("error when addressing nested element: {0:?}")]
    NestError(String),
    #[error("error to resolve Parcelable: {0:?}")]
    ResolveError(String),
}

pub type ParcelableResult<T> = Result<T, ParcelableError>;

pub trait Parcelable<F> {
    fn get_field(&mut self, name: &str) -> ParcelableResult<ParcelType<F>>;

    fn resolve(&mut self) -> ParcelableResult<ParcelType<F>>
    where
        Self: Sized + 'static,
    {
        let root = ParcelableResult::Ok(ParcelType::Parcelable(self));
        root
    }

    /// Get nested field by name, which is fields joined by dot
    /// for example: passing name "field1.somefield" will be the same
    /// as using `struct.field1.somefield`, by dynamically
    fn get_nested(&mut self, name: &str) -> ParcelableResult<ParcelType<F>>
    where
        Self: Sized + 'static,
    {
        let root = ParcelableResult::Ok(ParcelType::Parcelable(self));
        name.split('.')
            .fold(root, |s: ParcelableResult<ParcelType<F>>, field| match s? {
                ParcelType::Parcelable(p) => p.get_field(field),
                _ => Err(ParcelableError::NestError(format!(
                    "Failed to get field {field}. End of nestment"
                ))),
            })
    }
}

impl<F> Parcelable<F> for String {
    fn get_field(&mut self, _name: &str) -> ParcelableResult<ParcelType<F>> {
        todo!()
    }

    fn resolve(&mut self) -> ParcelableResult<ParcelType<F>>
    where
        Self: Sized + 'static,
    {
        Ok(ParcelType::Other(()))
    }
}

impl<F, T: Parcelable<F>> Parcelable<F> for Option<T> {
    fn get_field(&mut self, name: &str) -> ParcelableResult<ParcelType<F>> {
        Err(ParcelableError::FieldError(format!(
            "tried to get field {name}, but calls of get_field are not allowed on Option"
        )))
    }

    fn resolve(&mut self) -> crate::utils::parcelable::ParcelableResult<ParcelType<F>>
    where
        Self: Sized + 'static,
    {
        match self {
            Some(v) => Ok(v.resolve()?),
            None => Err(ParcelableError::ResolveError("Option was None".to_string())),
        }
    }
}

impl<F, V: Parcelable<F> + 'static> Parcelable<F> for HashMap<String, V> {
    fn get_field(&mut self, name: &str) -> ParcelableResult<ParcelType<F>> {
        match self.get_mut(name) {
            Some(v) => Ok(Parcelable::resolve(v)?),
            None => Err(ParcelableError::FieldError(format!(
                "tried to get value by key {name}, but this key does not exists"
            ))),
        }
    }
}

impl<F, T: Parcelable<F> + 'static> Parcelable<F> for Vec<T> {
    fn get_field(&mut self, name: &str) -> ParcelableResult<ParcelType<F>> {
        let index: usize = match name.parse() {
            Ok(index) => index,
            Err(err) => {
                return Err(ParcelableError::FieldError(format!(
                    "Failed to parse field name `{name}` as an array index, err: {err}"
                )))
            }
        };
        let veclen = self.len();
        let value = match self.get_mut(index) {
            Some(value) => value,
            None => return Err(ParcelableError::FieldError(format!("Failed to get vec element with index {index}, probably out of bound (vec len: {veclen})"))),
        };

        Parcelable::resolve(value)
    }
}
