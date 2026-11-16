import QtQuick
import QtQuick.Window
import org.kde.layershell as LayerShell
import org.kyko.app

// The drop-down: a layer-shell surface on the top edge that slides in and out. The surface is
// as tall as the panel plus its gap and stays put; the panel moves inside it, since a layer
// surface's margins cannot be animated from QML.
Window {
    id: root

    property bool shown: false
    property bool pinned: false
    // Set once the surface has had focus: only then does losing it count.
    property bool hadFocus: false

    readonly property bool sheet: ctl.setting("sheet") === 1
    readonly property int gap: sheet ? 0 : ctl.setting("gap_out")
    readonly property int panelH: Math.round(Screen.height * ctl.setting("height_pct") / 100)
    readonly property bool animated: ctl.setting("animations") === 1
    property color bg: hex(ctl.setting("background"))
    property color accent: hex(ctl.setting("accent"))

    function hex(v) { return "#" + ("000000" + v.toString(16)).slice(-6) }

    function show() {
        shown = true
    }

    function hide() {
        shown = false
    }

    width: sheet ? Screen.width : Math.round(Screen.width * ctl.setting("width_pct") / 100)
    height: panelH + gap
    visible: false
    color: "transparent"
    flags: Qt.FramelessWindowHint

    LayerShell.Window.layer: ctl.setting("above_fullscreen") === 1 ? LayerShell.Window.LayerOverlay : LayerShell.Window.LayerTop
    LayerShell.Window.anchors: LayerShell.Window.AnchorTop
    // Exclusive until the surface has focus, then on demand, so a pinned kyko can lose focus to
    // another window and stay open.
    LayerShell.Window.keyboardInteractivity: root.active ? LayerShell.Window.KeyboardInteractivityOnDemand : LayerShell.Window.KeyboardInteractivityExclusive
    LayerShell.Window.scope: "kyko"
    // The island sits below uQ's bar; the sheet covers it.
    LayerShell.Window.exclusionZone: sheet ? -1 : 0

    onShownChanged: {
        ctl.setVisible(shown)
        if (shown) {
            hadFocus = false
            visible = true
            requestActivate()
            grid.forceActiveFocus()
        }
        slide.to = shown ? gap : -panelH
        if (animated) {
            slide.restart()
        } else {
            panel.y = slide.to
            if (!shown)
                visible = false
        }
    }

    onActiveChanged: {
        if (active) {
            hadFocus = true
        } else if (shown && hadFocus && !pinned && ctl.setting("hide_on_focus_loss") === 1) {
            hide()
        }
    }

    NumberAnimation {
        id: slide
        target: panel
        property: "y"
        duration: 150
        easing.type: Easing.OutCubic
        onFinished: if (!root.shown) root.visible = false
    }

    Controller {
        id: ctl
        onCommand: function (code) {
            if (code === 0) root.shown ? root.hide() : root.show()
            else if (code === 1) root.show()
            else root.hide()
        }
        onLookChanged: {
            root.bg = root.hex(ctl.setting("background"))
            root.accent = root.hex(ctl.setting("accent"))
        }
    }

    Rectangle {
        id: panel
        y: -root.panelH
        width: parent.width
        height: root.panelH
        radius: ctl.setting("radius")
        color: root.bg
        border.width: ctl.setting("border_width")
        border.color: root.accent

        GridView {
            id: grid
            anchors.fill: parent
            anchors.margins: 8 + panel.border.width
            focus: true
        }

        Text {
            anchors.centerIn: parent
            visible: grid.notice.length > 0
            color: "#fb4934"
            text: grid.notice
            font.pixelSize: 16
        }
    }
}
