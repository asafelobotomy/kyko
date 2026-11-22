import QtQuick
import QtQuick.Window
import org.kde.layershell as LayerShell
import org.kyko.app

// The drop-down: one layer-shell surface over the screen's free area (below uQ's bar), holding
// the dimmed backdrop and the panel. The panel slides in from its edge (`window.position`: top,
// bottom, left, right) or fades and grows in the middle (center); a layer surface's margins
// cannot be animated, so the panel moves inside the surface instead. Clicks reach the windows
// beneath outside the panel unless the backdrop is dimmed. The panel's free edges drag to
// resize it, and the size is written back to config.toml for that position.
Window {
    id: root

    property bool shown: false
    property bool pinned: false
    // Set once the surface has had focus: only then does losing it count.
    property bool hadFocus: false

    // The settings, read again when the config or uQ's theme changes.
    property int pos: 0
    property bool sheet: false
    property int gapOut: 0
    property int wPct: 88
    property int hPct: 45
    property int dimPct: 40
    property bool aboveFullscreen: true
    property int radius: 10
    property int borderW: 1
    // uQ's motion tokens: 0 under Reduce motion or with `window.animations` off.
    property int mFast: 100
    property int mBase: 140
    property int mSlow: 220
    property color bg: "#000000"
    property color accent: "#ffffff"
    // `window.opacity_pct`: below 1 the desktop, and its blur, shows through the panel.
    property real opacityF: 1

    function hex(v) { return "#" + ("000000" + v.toString(16)).slice(-6) }

    function load() {
        pos = ctl.setting("position")
        sheet = ctl.setting("sheet") === 1
        gapOut = ctl.setting("gap_out")
        wPct = ctl.setting("width_pct")
        hPct = ctl.setting("height_pct")
        dimPct = ctl.setting("dim_pct")
        aboveFullscreen = ctl.setting("above_fullscreen") === 1
        radius = ctl.setting("radius")
        borderW = ctl.setting("border_width")
        mFast = ctl.setting("motion_fast")
        mBase = ctl.setting("motion_base")
        mSlow = ctl.setting("motion_slow")
        bg = hex(ctl.setting("background"))
        accent = hex(ctl.setting("accent"))
        opacityF = Math.max(30, Math.min(100, ctl.setting("opacity_pct"))) / 100
    }

    // 0 top, 1 bottom, 2 left, 3 right, 4 center.
    readonly property bool side: pos === 2 || pos === 3
    readonly property bool middle: pos === 4
    readonly property int gap: sheet && !middle ? 0 : gapOut
    // A sheet spans its whole edge; an island is a share of it.
    readonly property bool fullW: sheet && (pos === 0 || pos === 1)
    readonly property bool fullH: sheet && side
    // The config's size, in px of the free area, unless an edge is being dragged.
    property int dragW: -1
    property int dragH: -1
    readonly property int panelW: dragW >= 0 ? dragW : fullW ? width : Math.min(Math.round(width * wPct / 100), width - 2 * gap)
    readonly property int panelH: dragH >= 0 ? dragH : fullH ? height : Math.min(Math.round(height * hPct / 100), height - 2 * gap)
    // Where the panel rests while shown, and where it waits while hidden: past its edge.
    readonly property real homeX: pos === 2 ? gap : pos === 3 ? width - panelW - gap : Math.round((width - panelW) / 2)
    readonly property real homeY: pos === 0 ? gap : pos === 1 ? height - panelH - gap : Math.round((height - panelH) / 2)
    readonly property real awayX: pos === 2 ? -panelW - borderW : pos === 3 ? width + borderW : homeX
    readonly property real awayY: pos === 0 ? -panelH - borderW : pos === 1 ? height + borderW : homeY
    // 0 hidden, 1 shown: the panel's way in and out, and the backdrop's.
    property real t: 0
    property real dimT: 0
    readonly property bool dimmed: dimPct > 0 && !pinned

    // The size keys of this position in config.toml (kyko_core::config::Position::size_keys).
    function sizeKeys() {
        return side ? ["window.side_width_pct", "window.side_height_pct"]
            : middle ? ["window.center_width_pct", "window.center_height_pct"]
            : ["window.width_pct", "window.height_pct"]
    }

    function show() {
        shown = true
    }

    function hide() {
        shown = false
    }

    // Outside the panel the pointer reaches the windows beneath, unless the backdrop is dimmed:
    // then a click on it hides the drop-down.
    function updateInput() {
        if (dimmed)
            ctl.setInputRegion(root, 0, 0, 0, 0)
        else
            ctl.setInputRegion(root, Math.round(homeX), Math.round(homeY), Math.max(1, panelW), Math.max(1, panelH))
    }
    readonly property var inputShape: [dimmed, homeX, homeY, panelW, panelH, visible]
    onInputShapeChanged: if (visible) updateInput()

    width: Screen.width
    height: Screen.height
    visible: false
    color: "transparent"
    flags: Qt.FramelessWindowHint

    LayerShell.Window.layer: aboveFullscreen ? LayerShell.Window.LayerOverlay : LayerShell.Window.LayerTop
    LayerShell.Window.anchors: LayerShell.Window.AnchorTop | LayerShell.Window.AnchorBottom | LayerShell.Window.AnchorLeft | LayerShell.Window.AnchorRight
    // Exclusive until the surface has focus, then on demand, so a pinned kyko can lose focus to
    // another window and stay open.
    LayerShell.Window.keyboardInteractivity: root.active ? LayerShell.Window.KeyboardInteractivityOnDemand : LayerShell.Window.KeyboardInteractivityExclusive
    LayerShell.Window.scope: "kyko"
    // The island keeps out of uQ's bar; the sheet covers it.
    LayerShell.Window.exclusionZone: sheet ? -1 : 0

    Component.onCompleted: load()

    onShownChanged: {
        ctl.setVisible(shown)
        if (shown) {
            hadFocus = false
            visible = true
            updateInput()
            requestActivate()
            ws.focusPane()
        }
        // In: the panel settles at the base speed and the backdrop fades at the slow one. Out:
        // both get out of the way at the fast speed.
        slide.stop()
        slide.to = shown ? 1 : 0
        slide.duration = shown ? mBase : mFast
        slide.easing.type = shown ? Easing.OutCubic : Easing.InCubic
        fade.stop()
        fade.to = shown && dimmed ? 1 : 0
        fade.duration = shown ? mSlow : mFast
        if (slide.duration > 0) {
            slide.start()
        } else {
            t = slide.to
            if (!shown)
                visible = false
        }
        if (fade.duration > 0)
            fade.start()
        else
            dimT = fade.to
    }

    onDimmedChanged: {
        fade.stop()
        fade.to = shown && dimmed ? 1 : 0
        fade.duration = mFast
        if (fade.duration > 0)
            fade.start()
        else
            dimT = fade.to
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
        target: root
        property: "t"
        onFinished: if (!root.shown) root.visible = false
    }

    NumberAnimation {
        id: fade
        target: root
        property: "dimT"
        easing.type: Easing.OutCubic
    }

    Controller {
        id: ctl
        onCommand: function (code) {
            if (code === 0) root.shown ? root.hide() : root.show()
            else if (code === 1) root.show()
            else root.hide()
        }
        onLookChanged: root.load()
    }

    // The backdrop: the screen behind, darkened. A click on it hides the drop-down.
    Rectangle {
        id: backdrop
        anchors.fill: parent
        color: "black"
        opacity: root.dimT * root.dimPct / 100
        visible: opacity > 0

        MouseArea {
            anchors.fill: parent
            enabled: root.dimmed && root.shown
            onClicked: root.hide()
        }
    }

    Rectangle {
        id: panel
        objectName: "panel"
        x: root.awayX + (root.homeX - root.awayX) * root.t
        y: root.awayY + (root.homeY - root.awayY) * root.t
        width: root.panelW
        height: root.panelH
        // In the middle it fades and grows from uQ's popup scale instead of sliding.
        opacity: root.middle ? root.t : 1
        scale: root.middle && root.mBase > 0 ? 0.96 + 0.04 * root.t : 1
        radius: root.radius
        color: Qt.rgba(root.bg.r, root.bg.g, root.bg.b, root.opacityF)
        border.width: root.borderW
        border.color: root.accent

        Workspace {
            id: ws
            ctl: ctl
            anchors.fill: parent
            anchors.margins: 8 + panel.border.width
        }

        // The free edges resize the panel. Edges across a centred axis move both sides, so the
        // panel stays centred.
        Repeater {
            model: [
                // edge, axis (0 width, 1 height), sign, both sides
                { edge: "top", axis: 1, sign: -1, on: root.pos === 1 || root.pos === 4 || (root.side && !root.fullH) },
                { edge: "bottom", axis: 1, sign: 1, on: root.pos === 0 || root.pos === 4 || (root.side && !root.fullH) },
                { edge: "left", axis: 0, sign: -1, on: root.pos === 3 || root.pos === 4 || (!root.side && !root.middle && !root.fullW) },
                { edge: "right", axis: 0, sign: 1, on: root.pos === 2 || root.pos === 4 || (!root.side && !root.middle && !root.fullW) }
            ]
            delegate: MouseArea {
                id: grip
                required property var modelData
                readonly property bool centred: modelData.axis === 0 ? !root.side : (root.side || root.middle)
                property real start: 0
                property real startSize: 0
                visible: modelData.on
                hoverEnabled: true
                cursorShape: modelData.axis === 0 ? Qt.SizeHorCursor : Qt.SizeVerCursor
                x: modelData.edge === "right" ? panel.width - 8 : 0
                y: modelData.edge === "bottom" ? panel.height - 8 : 0
                width: modelData.axis === 0 ? 8 : panel.width
                height: modelData.axis === 1 ? 8 : panel.height
                onPressed: function (m) {
                    const p = mapToItem(null, m.x, m.y)
                    start = modelData.axis === 0 ? p.x : p.y
                    startSize = modelData.axis === 0 ? root.panelW : root.panelH
                }
                onPositionChanged: function (m) {
                    if (!pressed)
                        return
                    const p = mapToItem(null, m.x, m.y)
                    const d = ((modelData.axis === 0 ? p.x : p.y) - start) * modelData.sign * (centred ? 2 : 1)
                    const full = modelData.axis === 0 ? root.width : root.height
                    const v = Math.round(Math.max(full * 0.2, Math.min(full - 2 * root.gap, startSize + d)))
                    if (modelData.axis === 0)
                        root.dragW = v
                    else
                        root.dragH = v
                }
                onReleased: {
                    const keys = root.sizeKeys()
                    if (modelData.axis === 0 && root.dragW >= 0) {
                        const pct = Math.max(20, Math.min(100, Math.round(root.dragW * 100 / root.width)))
                        root.wPct = pct
                        ctl.prefsSet(keys[0], JSON.stringify(pct))
                    } else if (modelData.axis === 1 && root.dragH >= 0) {
                        const pct = Math.max(20, Math.min(100, Math.round(root.dragH * 100 / root.height)))
                        root.hPct = pct
                        ctl.prefsSet(keys[1], JSON.stringify(pct))
                    }
                    root.dragW = -1
                    root.dragH = -1
                }
            }
        }
    }
}
