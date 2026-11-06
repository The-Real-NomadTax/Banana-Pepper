//! Cabinet actions and default bindings, rendered by the shared controls UI.
#[allow(unused_imports)]
use banana_input::controls::{any, one, pad, Game, KeySpace};

pub static CONTROLS: Game = Game {id:"ford",title:"Ford Racing Full Blown",keys:KeySpace::DirectInput,mouse_buttons:false,note:"Keyboard steering ramps and recentres. Controllers (XInput): left stick steers, triggers bound to gas/brake are analog. The cabinet adapter runs on Windows and Wine; Escape exits.",player_two:&[],actions:&[
        any("steer_left","Steer left",1,&["Left","PadLeft"]),any("steer_right","Steer right",2,&["Right","PadRight"]),any("gas","Gas / menu confirm",3,&["Up","PadRT"]),any("brake","Brake",4,&["Down","PadLT"]),
        any("start1","Start",5,&["Enter","1","PadStart","PadA"]),any("view","Change view",6,&["V","PadY"]),any("gear_up","Shift up",7,&["Space","PadRB"]),any("gear_down","Shift down",8,&["X","PadLB"]),any("coin1","Coin",9,&["5","PadBack"]),
    ]};
