//! Application use cases and ports owned by the application layer.

use einkaufio_domain::{ItemId, ItemName, ItemNote, ItemQuantity, ItemUnit, ItemValueError, ListId, ShoppingItem, UserId};
use crate::ports::{ItemIdGenerator, RepositoryError, ShoppingItemRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddShoppingItemCommand { pub actor_id:String, pub list_id:String, pub expected_list_version:u64, pub name:String, pub quantity:Option<String>, pub unit:Option<String>, pub note:Option<String>, pub position:u32 }
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShoppingItemError { InvalidActor, InvalidListId, InvalidItemId, InvalidName(ItemValueError), InvalidQuantity(ItemValueError), InvalidUnit(ItemValueError), InvalidNote(ItemValueError), InvalidExpectedVersion, NotFound, Conflict, PersistenceUnavailable }
pub struct AddShoppingItem<'a>{ repository:&'a dyn ShoppingItemRepository, id_generator:&'a dyn ItemIdGenerator }
impl<'a> AddShoppingItem<'a>{
 pub fn new(repository:&'a dyn ShoppingItemRepository,id_generator:&'a dyn ItemIdGenerator)->Self{Self{repository,id_generator}}
 pub fn execute(&self,c:AddShoppingItemCommand)->Result<(ShoppingItem,u64),ShoppingItemError>{
  let actor=UserId::try_new(c.actor_id).map_err(|_|ShoppingItemError::InvalidActor)?;
  let list=ListId::try_new(c.list_id).map_err(|_|ShoppingItemError::InvalidListId)?;
  if c.expected_list_version==0{return Err(ShoppingItemError::InvalidExpectedVersion)}
  let name=ItemName::try_new(c.name).map_err(ShoppingItemError::InvalidName)?;
  let quantity=c.quantity.map(ItemQuantity::try_new).transpose().map_err(ShoppingItemError::InvalidQuantity)?;
  let unit=c.unit.map(ItemUnit::try_new).transpose().map_err(ShoppingItemError::InvalidUnit)?;
  let note=c.note.map(ItemNote::try_new).transpose().map_err(ShoppingItemError::InvalidNote)?;
  let id=ItemId::try_new(self.id_generator.generate()).map_err(|_|ShoppingItemError::InvalidItemId)?;
  let item=ShoppingItem::new(id,name,quantity,unit,note,c.position,actor);
  let version=self.repository.add_item(list.as_str(),c.expected_list_version,item.clone()).map_err(map_error)?;
  Ok((item,version))
 }
}
#[derive(Clone,Debug,Eq,PartialEq)] pub struct SetShoppingItemCheckedCommand{pub list_id:String,pub item_id:String,pub expected_item_version:u64,pub checked:bool}
pub struct SetShoppingItemChecked<'a>{repository:&'a dyn ShoppingItemRepository}
impl<'a> SetShoppingItemChecked<'a>{pub fn new(repository:&'a dyn ShoppingItemRepository)->Self{Self{repository}} pub fn execute(&self,c:SetShoppingItemCheckedCommand)->Result<(ShoppingItem,u64),ShoppingItemError>{let l=ListId::try_new(c.list_id).map_err(|_|ShoppingItemError::InvalidListId)?;let i=ItemId::try_new(c.item_id).map_err(|_|ShoppingItemError::InvalidItemId)?;if c.expected_item_version==0{return Err(ShoppingItemError::InvalidExpectedVersion)}self.repository.set_item_checked(l.as_str(),&i,c.expected_item_version,c.checked).map_err(map_error)}}
#[derive(Clone,Debug,Eq,PartialEq)] pub struct DeleteShoppingItemCommand{pub list_id:String,pub item_id:String,pub expected_item_version:u64}
pub struct DeleteShoppingItem<'a>{repository:&'a dyn ShoppingItemRepository}
impl<'a> DeleteShoppingItem<'a>{pub fn new(repository:&'a dyn ShoppingItemRepository)->Self{Self{repository}} pub fn execute(&self,c:DeleteShoppingItemCommand)->Result<u64,ShoppingItemError>{let l=ListId::try_new(c.list_id).map_err(|_|ShoppingItemError::InvalidListId)?;let i=ItemId::try_new(c.item_id).map_err(|_|ShoppingItemError::InvalidItemId)?;if c.expected_item_version==0{return Err(ShoppingItemError::InvalidExpectedVersion)}self.repository.delete_item(l.as_str(),&i,c.expected_item_version).map_err(map_error)}}
fn map_error(e:RepositoryError)->ShoppingItemError{match e{RepositoryError::NotFound=>ShoppingItemError::NotFound,RepositoryError::Conflict|RepositoryError::AlreadyExists=>ShoppingItemError::Conflict,RepositoryError::Unavailable=>ShoppingItemError::PersistenceUnavailable}}
#[cfg(test)] mod tests{
 use super::*; use std::cell::RefCell; use einkaufio_domain::{ItemName,ShoppingItem}; use crate::ports::RepositoryError;
 #[derive(Default)] struct Repo{added:RefCell<Vec<ShoppingItem>>,failure:Option<RepositoryError>}
 impl ShoppingItemRepository for Repo{fn add_item(&self,_:&str,v:u64,i:ShoppingItem)->Result<u64,RepositoryError>{if let Some(e)=&self.failure{return Err(e.clone())}self.added.borrow_mut().push(i);Ok(v+1)}fn set_item_checked(&self,_:&str,id:&ItemId,v:u64,c:bool)->Result<(ShoppingItem,u64),RepositoryError>{if let Some(e)=&self.failure{return Err(e.clone())}let mut i=ShoppingItem::new(id.clone(),ItemName::try_new("Milch").unwrap(),None,None,None,0,UserId::try_new("u").unwrap());i.set_checked(c);Ok((i,v+1))}fn delete_item(&self,_:&str,_:&ItemId,v:u64)->Result<u64,RepositoryError>{if let Some(e)=&self.failure{return Err(e.clone())}Ok(v+1)}}
 struct Id;impl ItemIdGenerator for Id{fn generate(&self)->String{"item-1".into()}}
 fn cmd()->AddShoppingItemCommand{AddShoppingItemCommand{actor_id:"user-1".into(),list_id:"list-1".into(),expected_list_version:2,name:" Milch ".into(),quantity:Some("2".into()),unit:Some("Liter".into()),note:None,position:0}}
 #[test]fn adds_and_versions_item(){let r=Repo::default();let(i,v)=AddShoppingItem::new(&r,&Id).execute(cmd()).unwrap();assert_eq!(i.name().as_str(),"Milch");assert_eq!(v,3);assert_eq!(r.added.borrow().len(),1)}
 #[test]fn validates_before_persisting(){let r=Repo::default();let c=AddShoppingItemCommand{name:" ".into(),..cmd()};assert!(matches!(AddShoppingItem::new(&r,&Id).execute(c),Err(ShoppingItemError::InvalidName(_))));assert!(r.added.borrow().is_empty())}
 #[test]fn rejects_zero_version(){let r=Repo::default();let c=AddShoppingItemCommand{expected_list_version:0,..cmd()};assert_eq!(AddShoppingItem::new(&r,&Id).execute(c),Err(ShoppingItemError::InvalidExpectedVersion))}
 #[test]fn maps_conflicts(){let r=Repo{failure:Some(RepositoryError::Conflict),..Default::default()};assert_eq!(AddShoppingItem::new(&r,&Id).execute(cmd()),Err(ShoppingItemError::Conflict))}
 #[test]fn checks_item(){let r=Repo::default();let(i,v)=SetShoppingItemChecked::new(&r).execute(SetShoppingItemCheckedCommand{list_id:"l".into(),item_id:"i".into(),expected_item_version:1,checked:true}).unwrap();assert!(i.checked());assert_eq!(v,2)}
 #[test]fn deletes_item(){let r=Repo::default();assert_eq!(DeleteShoppingItem::new(&r).execute(DeleteShoppingItemCommand{list_id:"l".into(),item_id:"i".into(),expected_item_version:1}).unwrap(),2)}
 #[test]fn maps_missing_and_unavailable(){let r=Repo{failure:Some(RepositoryError::NotFound),..Default::default()};assert_eq!(DeleteShoppingItem::new(&r).execute(DeleteShoppingItemCommand{list_id:"l".into(),item_id:"i".into(),expected_item_version:1}),Err(ShoppingItemError::NotFound));let r=Repo{failure:Some(RepositoryError::Unavailable),..Default::default()};assert_eq!(DeleteShoppingItem::new(&r).execute(DeleteShoppingItemCommand{list_id:"l".into(),item_id:"i".into(),expected_item_version:1}),Err(ShoppingItemError::PersistenceUnavailable))}
}