//! Non-destructive source-image adjustments. Outputs are source bitmaps, never layer texel storage.
use glam::{Vec2, Vec3};
use std::sync::Arc;
use subcraft_image::{Bitmap, Wrap};
use crate::starter::Kind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Address { #[default] Repeat, Mirror, Clamp }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub strength: f32, pub flip_green: bool, pub invert: bool,
    pub brightness: f32, pub contrast: f32, pub scale: f32, pub rotation: f32,
    pub wrap: Address, pub seamless: f32,
}
impl Default for Settings {
    fn default() -> Self { Self { strength: 1.0, flip_green: false, invert: false, brightness: 0.0, contrast: 1.0, scale: 1.0, rotation: 0.0, wrap: Address::Repeat, seamless: 0.0 } }
}
impl Settings {
    pub fn validate(&self) -> Result<(), &'static str> {
        for (v, lo, hi) in [(self.strength,0.0,4.0),(self.brightness,-1.0,1.0),(self.contrast,0.0,4.0),(self.scale,0.1,10.0),(self.rotation,-180.0,180.0),(self.seamless,0.0,0.25)] {
            if !v.is_finite() || !(lo..=hi).contains(&v) { return Err("invalid image adjustment"); }
        }
        Ok(())
    }
    pub fn sample(&self, image: &Bitmap, kind: Kind, uv: Vec2) -> [f32; 4] {
        let (s,c) = self.rotation.to_radians().sin_cos();
        let p = (uv - Vec2::splat(0.5)) * self.scale;
        let p = Vec2::new(c*p.x+s*p.y,-s*p.x+c*p.y) + Vec2::splat(0.5);
        let address = |v: f32| match self.wrap {
            Address::Repeat => (v, 1.0), Address::Clamp => (v.clamp(0.0,1.0),1.0),
            Address::Mirror => { let v=v.rem_euclid(2.0); if v>1.0 {(2.0-v,-1.0)} else {(v,1.0)} }
        };
        let (u,sx)=address(p.x); let (v,sy)=address(p.y);
        let wrap=if self.wrap==Address::Repeat { Wrap::Repeat } else { Wrap::Clamp };
        let mut t=image.sample(u,v,wrap,false);
        if self.seamless>0.0 {
            let blend=|v: f32| ((self.seamless-v.min(1.0-v))/self.seamless).clamp(0.0,1.0)*0.5;
            let (a,b)=(blend(u.rem_euclid(1.0)),blend(v.rem_euclid(1.0)));
            let x=image.sample(1.0-u.rem_euclid(1.0),v,wrap,false);
            let y=image.sample(u,1.0-v.rem_euclid(1.0),wrap,false);
            let xy=image.sample(1.0-u.rem_euclid(1.0),1.0-v.rem_euclid(1.0),wrap,false);
            for i in 0..4 { t[i]=t[i]*(1.0-a)*(1.0-b)+x[i]*a*(1.0-b)+y[i]*(1.0-a)*b+xy[i]*a*b; }
        }
        if kind==Kind::Normal {
            let mut x=(t[0]*2.0-1.0)*self.strength*sx;
            let mut y=(t[1]*2.0-1.0)*self.strength*sy;
            if self.flip_green { y = -y; }
            if self.invert { x = -x; y = -y; }
            let n=Vec3::new(c*x-s*y,s*x+c*y,t[2]*2.0-1.0).try_normalize().unwrap_or(Vec3::Z);
            return [n.x*0.5+0.5,n.y*0.5+0.5,n.z*0.5+0.5,1.0];
        }
        let adjust=|v: f32| { let v=((v-0.5)*self.contrast+0.5+self.brightness).clamp(0.0,1.0); if self.invert {1.0-v} else {v} };
        if kind==Kind::Alpha { let g=adjust(t[0]*0.2126+t[1]*0.7152+t[2]*0.0722)*t[3]; [g,g,g,1.0] }
        else { [adjust(t[0]),adjust(t[1]),adjust(t[2]),t[3]] }
    }
}

/// Full-size apply and thumbnail previews use the same math, with bounded pixel work per frame.
pub struct ImageJob { source: Arc<Bitmap>, kind: Kind, settings: Settings, side: u32, pixel: usize, data: Vec<u8> }
impl ImageJob {
    pub fn new(source: Arc<Bitmap>, kind: Kind, settings: Settings, side: u32) -> Result<Self, &'static str> {
        settings.validate()?; if side==0 || side>4096 { return Err("invalid preview size"); }
        Ok(Self { source, kind, settings, side, pixel:0, data:Vec::with_capacity((side*side) as usize*4) })
    }
    pub fn progress(&self) -> f32 { self.pixel as f32/(self.side*self.side) as f32 }
    pub fn is_done(&self) -> bool { self.pixel==(self.side*self.side) as usize }
    pub fn step(&mut self, budget: usize) {
        let comps=if self.kind==Kind::Alpha {1} else if self.kind==Kind::Normal {3} else {4};
        let end=(self.pixel+budget).min((self.side*self.side) as usize);
        while self.pixel<end {
            let x=self.pixel as u32%self.side; let y=self.pixel as u32/self.side;
            let t=self.settings.sample(&self.source,self.kind,Vec2::new((x as f32+0.5)/self.side as f32,(y as f32+0.5)/self.side as f32));
            self.data.extend(t[..comps].iter().map(|x| (x.clamp(0.0,1.0)*255.0).round() as u8)); self.pixel+=1;
        }
    }
    pub fn finish(mut self) -> Option<Bitmap> {
        if !self.is_done() { return None; }
        let n=if self.kind==Kind::Alpha {1} else if self.kind==Kind::Normal {3} else {4};
        if self.settings.seamless>0.0 {
            let mut pair=|a: usize,b: usize| {
                let mut v=[0.0;4];
                for (c,x) in v.iter_mut().enumerate().take(n) { *x=(self.data[a*n+c] as f32+self.data[b*n+c] as f32)/510.0; }
                if self.kind==Kind::Normal { let p=(Vec3::from_slice(&v[..3])*2.0-Vec3::ONE).try_normalize().unwrap_or(Vec3::Z)*0.5+Vec3::splat(0.5); v[..3].copy_from_slice(&p.to_array()); }
                for (c,x) in v.iter().enumerate().take(n) { let x=(x*255.0).round() as u8; self.data[a*n+c]=x; self.data[b*n+c]=x; }
            };
            let s=self.side as usize;
            for y in 0..s {pair(y*s,y*s+s-1);} for x in 0..s {pair(x,(s-1)*s+x);}
        }
        Some(Bitmap::new_u8(self.side,self.side,n as u8,self.data))
    }
}

/// A lit sphere for normals or an example stamped stroke for alpha tips.
pub fn example(image: &Bitmap, kind: Kind) -> Bitmap {
    let (w,h)=if kind==Kind::Alpha {(256,72)} else {(128,128)};
    let mut bytes=Vec::with_capacity(w*h*3);
    for y in 0..h { for x in 0..w {
        let rgb=if kind==Kind::Alpha {
            let mut a=0.0f32;
            for i in 0..32 { let cx=20.0+i as f32*7.0; let cy=36.0+(i as f32*0.24).sin()*12.0;
                let u=(x as f32-cx)/40.0+0.5; let v=(y as f32-cy)/40.0+0.5;
                if (0.0..=1.0).contains(&u)&&(0.0..=1.0).contains(&v) { a=a.max(image.sample(u,v,Wrap::Clamp,false)[0]); }
            } [0.12+a*0.85;3]
        } else {
            let u=(x as f32+0.5)/w as f32; let v=(y as f32+0.5)/h as f32;
            let p=Vec2::new(u*2.0-1.0,1.0-v*2.0);
            if p.length_squared()>1.0 { [0.12;3] } else {
                let base=Vec3::new(p.x,p.y,(1.0-p.length_squared()).sqrt()); let t=Vec3::new(base.z,0.0,-base.x).normalize_or_zero(); let b=base.cross(t);
                let pixel=image.sample(u,v,Wrap::Repeat,false); let n=(t*(pixel[0]*2.0-1.0)+b*(pixel[1]*2.0-1.0)+base*(pixel[2]*2.0-1.0)).normalize_or_zero();
                let light=Vec3::new(-0.4,0.5,0.8).normalize(); let l=0.06+0.65*n.dot(light).max(0.0)+0.35*n.dot((light+Vec3::Z).normalize()).max(0.0).powi(32);
                [subcraft_core::color::linear_to_srgb(l);3]
            }
        };
        bytes.extend(rgb.map(|v| (v.clamp(0.0,1.0)*255.0).round() as u8));
    } }
    Bitmap::new_u8(w as u32,h as u32,3,bytes)
}
