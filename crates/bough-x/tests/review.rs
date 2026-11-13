//! The independent review's probes for the core (review of 2026-11-12, findings C01 to C13), as tests. Each
//! failed against the core as it was; `world_rejects_different_view_type` is restated: a world handed a view of
//! another type than it was mounted with refuses it at the entry point (the review's probe expected the other
//! view's handler to run, which no world mounted for `A` can do).
use bough_x::prelude::*;
use bough_x::soft::{Soft,Immediate};
use bough_x::event::{Button,U_FOCUS};
lens!(Offset:i32=0);
lens!(Scroll:i32=1);
struct Moving { u:U, clicks:u32 }
impl View for Moving {
 fn block(&self)->&U{&self.u}
 fn view<S:Sink<Self>>(&self,s:&mut S){s.row(||([Rect::new(0,0,10,10)],Outline::<1,_,_>(Kc::<0xffffffff>,un::<Offset>().map(|x|(x,0))).on_click(|v:&mut Moving|v.clicks+=1)));}
}
#[test] fn moving_target_is_picked_at_visible_position(){let mut v=Moving{u:U::default(),clicks:0};v.u.set::<Offset>(40);let mut w=World::<Soft>::new(100,100,2);w.mount(&v);w.click(&mut v,40,2);assert_eq!(v.clicks,1);}
#[test] fn pointer_updates_expression_words(){let mut v=Moving{u:U::default(),clicks:0};let mut w=World::<Soft>::new(100,100,2);w.mount(&v);w.pointer(&mut v,35,41);assert_eq!((w.shown.w[bough_x::expr::U_PTR_X],w.shown.w[bough_x::expr::U_PTR_Y]),(35,41));}
struct Rows {u:U,shape:Vec<Rect>,items:Rel<u32>, clicked:u32, h:i32}
impl View for Rows{
 fn block(&self)->&U{&self.u}
 fn view<S:Sink<Self>>(&self,s:&mut S){s.scope(Space{clip:Rect::new(0,0,100,self.h),base:(0,0),scroll:Scroll::SLOT as u8},|s|s.list(&List{n:self.items.len() as u32,row_h:10,x:0,y:0,shape:&self.shape,edits:&self.items.journal},|r|{let id=*self.items.get(r);Fill(Kc::<0xffff0000>).on_click(move|v:&mut Rows|v.clicked=id).focusable()}));}
}
fn rows()->Rows{Rows{u:U::default(),shape:vec![Rect::new(0,0,20,10)],items:Rel::new((0..200).collect()),clicked:999,h:30}}
#[test]fn in_place_shape_changes_update_layout(){let mut v=rows();let mut w=World::<Soft>::new(100,100,2);w.mount(&v);v.shape[0].x1=60;w.redescribe(&v);let mut full=Soft::new(100,100);w.paint(&mut full);let mut im=Soft::new(100,100);v.view(&mut Immediate::new(&mut im,&w.shown));assert!(full.px==im.px,"same allocation's edited layout kept stale rects");}
#[test]fn focus_follows_item_through_insert(){let mut v=rows();let mut w=World::<Soft>::new(100,100,2);w.mount(&v);w.click(&mut v,5,15);assert_eq!(v.clicked,1);v.items.insert(0,900);w.settle(&v);let place=w.shown.w[U_FOCUS];w.key_down(&mut v,Key::Enter,Mods::default());let logical=w.focused().unwrap();assert_eq!(logical.2,2,"focused item 1 moved to row 2; place={place}");}
#[test]fn press_is_cancelled_when_slot_reused(){let mut v=rows();let mut w=World::<Soft>::new(100,100,2);w.mount(&v);w.pointer_down(&mut v,5,5,Button::Primary);v.u.set::<Scroll>(640);w.settle(&v);w.pointer_up(&mut v,5,5,Button::Primary);assert_eq!(v.clicked,999,"press on item 0 activated item 64 after ring reuse");}
#[test]fn window_can_grow(){let mut v=rows();let mut w=World::<Soft>::new(100,2000,2);w.mount(&v);v.h=1200;w.redescribe(&v);}
#[repr(C)]struct A{u:U,n:u32}
#[repr(C)]struct B{u:U,n:u32}
impl View for A{fn block(&self)->&U{&self.u}fn view<S:Sink<Self>>(&self,s:&mut S){s.row(||([Rect::new(0,0,30,30)],Fill(Kc::<0xffffffff>).on_click(|v:&mut A|v.n+=1)));}}
impl View for B{fn block(&self)->&U{&self.u}fn view<S:Sink<Self>>(&self,s:&mut S){s.row(||([Rect::new(0,0,30,30)],Fill(Kc::<0xffffffff>).on_click(|v:&mut B|v.n+=100)));}}
#[test]
#[should_panic(expected = "another view type")]
fn world_rejects_different_view_type(){let a=A{u:U::default(),n:0};let mut b=B{u:U::default(),n:0};let mut w=World::<Soft>::new(100,100,0);w.mount(&a);w.click(&mut b,5,5);assert_eq!(b.n,0,"A's trampoline was called on B");}
struct Overlap{u:U, moved:bool}
impl View for Overlap{fn block(&self)->&U{&self.u}fn view<S:Sink<Self>>(&self,s:&mut S){s.row(||([Rect::new(0,0,10,10),Rect::new(20,0,10,10),Rect::new(if self.moved{20}else{40},0,10,10)],(Fill(Kc::<0xffff0000>),Fill(Kc::<0xff00ff00>),Fill(Kc::<0xffff0000>))));}}
#[test]fn changed_geometry_preserves_paint_order(){let mut v=Overlap{u:U::default(),moved:false};let mut w=World::<Soft>::new(100,100,0);w.mount(&v);v.moved=true;w.redescribe(&v);let mut full=Soft::new(100,100);w.paint(&mut full);let mut im=Soft::new(100,100);v.view(&mut Immediate::new(&mut im,&w.shown));assert!(full.px==im.px,"columns grouped when disjoint reorder later overlap");}
fn list_at_same_site<V:'static,S:Sink<V>,R:Row<V>+Copy>(s:&mut S,r:R){s.list(&List{n:1,row_h:10,x:0,y:0,shape:&[Rect::new(0,0,10,10)],edits:&FIXED},|_|r);}
struct ChangedType{u:U, green:bool}
impl View for ChangedType{fn block(&self)->&U{&self.u}fn view<S:Sink<Self>>(&self,s:&mut S){if self.green{list_at_same_site(s,Fill(Kc::<0xff00ff00>));}else{list_at_same_site(s,Fill(Kc::<0xffff0000>));}}}
#[test]fn same_size_list_type_change_is_not_silently_accepted(){let mut v=ChangedType{u:U::default(),green:false};let mut w=World::<Soft>::new(30,30,0);w.mount(&v);v.green=true;w.redescribe(&v);let mut px=Soft::new(30,30);w.paint(&mut px);assert_eq!(px.px[0],0xff00ff00,"old archetype's constant color survived a same-size row-type change");}
struct WrappingTime{u:U}
impl View for WrappingTime{fn block(&self)->&U{&self.u}fn view<S:Sink<Self>>(&self,s:&mut S){s.row(||([Rect::new(0,0,10,10)],Fill(Tween::<100,_,_,_>(Kc::<0xff000000>,Kc::<0xffffffff>,rec(u32::MAX-10)))));}}
#[test]fn active_ramp_survives_clock_wrap(){let v=WrappingTime{u:U::default()};let mut w=World::<Soft>::new(30,30,0);w.tick(u32::MAX-5);w.mount(&v);assert!(!w.tiles.sweeps.is_empty(),"5ms into a 100ms ramp was classified at rest because its deadline wrapped");}
