// Usage (screen coordinates = window origin from `wid` + window-relative point):
//   mouse click <x> <y>
//   mouse rclick <x> <y>                     (right click: context menus)
//   mouse move  <x> <y>                      (hover; gpui needs a real move first)
//   mouse drag  <x0> <y> <x1> <steps> [<windowNumber> <outPrefix>]
//                screenshots after each step when windowNumber/outPrefix are given
//   mouse dragto <x0> <y0> <x1> <y1>        (drag & drop, e.g. sidebar rows)
import CoreGraphics
import Foundation

let a = CommandLine.arguments
func post(_ type: CGEventType, _ x: Double, _ y: Double, _ button: CGMouseButton = .left) {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: button)!
        .post(tap: .cghidEventTap)
}

switch a[1] {
case "move":
    post(.mouseMoved, Double(a[2])!, Double(a[3])!)
case "click":
    let (x, y) = (Double(a[2])!, Double(a[3])!)
    for t in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] { post(t, x, y); usleep(80_000) }
case "rclick":
    let (x, y) = (Double(a[2])!, Double(a[3])!)
    post(.mouseMoved, x, y); usleep(80_000)
    for t in [CGEventType.rightMouseDown, .rightMouseUp] { post(t, x, y, .right); usleep(80_000) }
case "dragto":
    let (x0, y0, x1, y1) = (Double(a[2])!, Double(a[3])!, Double(a[4])!, Double(a[5])!)
    post(.mouseMoved, x0, y0); usleep(200_000)
    post(.leftMouseDown, x0, y0); usleep(100_000)
    for i in 1...12 {
        let f = Double(i) / 12
        post(.leftMouseDragged, x0 + (x1 - x0) * f, y0 + (y1 - y0) * f); usleep(40_000)
    }
    usleep(200_000)
    post(.leftMouseUp, x1, y1)
case "drag":
    let (x0, y, x1, steps) = (Double(a[2])!, Double(a[3])!, Double(a[4])!, Int(a[5])!)
    post(.mouseMoved, x0, y); usleep(200_000)
    post(.leftMouseDown, x0, y); usleep(100_000)
    for i in 1...steps {
        post(.leftMouseDragged, x0 + (x1 - x0) * Double(i) / Double(steps), y)
        usleep(30_000)
        if a.count > 7 {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
            p.arguments = ["-x", "-o", "-l\(a[6])", "\(a[7])\(i).png"]
            try? p.run(); p.waitUntilExit()
        }
    }
    post(.leftMouseUp, x1, y)
default:
    print("usage: mouse click|move|drag ...")
}
