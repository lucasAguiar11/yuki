// Usage (screen coordinates = window origin from `wid` + window-relative point):
//   mouse click <x> <y>
//   mouse move  <x> <y>                      (hover; gpui needs a real move first)
//   mouse drag  <x0> <y> <x1> <steps> [<windowNumber> <outPrefix>]
//                screenshots after each step when windowNumber/outPrefix are given
import CoreGraphics
import Foundation

let a = CommandLine.arguments
func post(_ type: CGEventType, _ x: Double, _ y: Double) {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: .left)!
        .post(tap: .cghidEventTap)
}

switch a[1] {
case "move":
    post(.mouseMoved, Double(a[2])!, Double(a[3])!)
case "click":
    let (x, y) = (Double(a[2])!, Double(a[3])!)
    for t in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] { post(t, x, y); usleep(80_000) }
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
