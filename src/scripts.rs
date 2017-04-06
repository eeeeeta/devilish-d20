use models::*;
use ketos::{Scope, ForeignValue};
use diesel::pg::PgConnection;
use std::rc::Rc;
use std::cell::RefCell;
use schema::players::dsl as pdsl;
use std::fmt;
use gm::MatrixClient;

pub struct Chat {
    pub inner: Rc<RefCell<MatrixClient>>
}
pub struct Database {
    pub inner: Rc<RefCell<PgConnection>>
}
impl fmt::Debug for Chat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[Matrix connection]")
    }
}
impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[database connection]")
    }
}
impl ForeignValue for Chat {
    fn type_name(&self) -> &'static str { "Chat" }
}
foreign_type_conversions! { Chat => "Chat" }
impl ForeignValue for Database {
    fn type_name(&self) -> &'static str { "Database" }
}
foreign_type_conversions! { Database => "Database" }

macro_rules! ketos_fns {
    ($impl:ident, $dsl:ident, $table:ident, $self:ty | $($name:ident, $typ:ty, $styp:ty),+) => {
        pub fn $impl(scope: &Scope) {
            $({
                use diesel::prelude::*;
                use ketos::Error as KetosError;
                use std::error::Error as StdError;
                fn getter(db: &Database, id: i32) -> Result<$typ, KetosError> {
                    let db = db.inner.borrow();
                    let res = $dsl::$table.filter($dsl::id.eq(id))
                        .get_result::<$self>(&*db).map_err(|e| Box::new(e) as Box<StdError>)?;
                    Ok(res.$name.into())
                }
                fn setter(db: &Database, id: i32, newval: $styp) -> Result<(), KetosError> {
                    let db = db.inner.borrow();
                    ::diesel::update($dsl::$table.filter($dsl::id.eq(id)))
                        .set($dsl::$name.eq(newval))
                        .execute(&*db).map_err(|e| Box::new(e) as Box<StdError>)?;
                    Ok(())
                }
                ketos_fn!{ scope => concat!(stringify!($table), "-get-", stringify!($name)) =>
                           fn getter(db: &Database, id: i32) -> $typ }

                ketos_fn!{ scope => concat!(stringify!($table), "-set-", stringify!($name)) =>
                           fn setter(db: &Database, id: i32, newval: $styp) -> () }
            })*
        }
    }
}
pub fn register_matrix(scope: &Scope, mx: Rc<RefCell<MatrixClient>>) {
    use ketos::Error as KetosError;
    use ketos::Value;
    use std::error::Error as StdError;
    scope.add_named_value("mx", Value::Foreign(Rc::new(Chat {
        inner: mx
    })));
    fn print(ch: &Chat, to: &str, msg: &str) -> Result<(), KetosError> {
        use gm::types::*;
        let m = Message::Notice { body: msg.into(), formatted_body: Some(msg.replace("\n", "<br/>")), format: Some("org.matrix.custom.html".into()) };
        ch.inner.borrow_mut().send(to, m).map_err(|e| Box::new(e) as Box<StdError>)?;
        ::std::thread::sleep(::std::time::Duration::from_millis(250));
        Ok(())
    }
    ketos_fn! { scope => "msg" => fn print(ch: &Chat, to: &str, msg: &str) -> () }
}
ketos_fns!(register_players, pdsl, players, Player |
           name, String, &str,
           typ, String, &str,
           armor_class, i32, i32,
           hit_points, i32, i32,
           strength, i32, i32,
           intelligence, i32, i32,
           dexterity, i32, i32,
           constitution, i32, i32,
           wisdom, i32, i32,
           charisma, i32, i32,
           initiative_bonus, i32, i32
);
impl ForeignValue for Player {
    fn type_name(&self) -> &'static str { "Player" }
}
foreign_type_conversions!{Player => "Player"}
