import QtQuick
import org.kyko.app

// The tabs and their panes: a tab bar, and the active tab's split tree of terminal grids. The
// session lives in Rust; the layout arrives as JSON and is read again whenever it changes.
Item {
    id: ws

    required property var ctl

    property var layout: ({ tabs: [], focused: 0, active: 0, notice: "", title: "" })
    property var paneIds: []
    property var tabIds: []
    property var dividerKeys: []
    readonly property string title: layout.title
    readonly property bool tabsBottom: ctl.setting("tabs_bottom") === 1
    readonly property int barH: 32
    property color bg: hex(ctl.setting("background"))
    property color fg: hex(ctl.setting("foreground"))
    property color accent: hex(ctl.setting("accent"))
    readonly property color dim: Qt.rgba(fg.r, fg.g, fg.b, 0.12)

    function hex(v) { return "#" + ("000000" + v.toString(16)).slice(-6) }

    function activeTab() {
        for (var i = 0; i < layout.tabs.length; ++i)
            if (layout.tabs[i].active) return layout.tabs[i]
        return null
    }

    function paneInfo(id) {
        var t = activeTab()
        if (t) for (var i = 0; i < t.panes.length; ++i) if (t.panes[i].id === id) return t.panes[i]
        return null
    }

    function dividerInfo(key) {
        var t = activeTab()
        if (t) for (var i = 0; i < t.dividers.length; ++i)
            if (t.dividers[i].tab + ":" + t.dividers[i].path === key) return t.dividers[i]
        return null
    }

    function tabInfo(id) {
        for (var i = 0; i < layout.tabs.length; ++i) if (layout.tabs[i].id === id) return layout.tabs[i]
        return null
    }

    // Repeaters rebuild their items when handed a new array, so one is only replaced when its
    // contents changed: the grids keep their atlases and focus across a rename.
    function same(a, b) { return JSON.stringify(a) === JSON.stringify(b) }

    function reload() {
        var text = ctl.layout()
        if (text.length === 0) return
        var l = JSON.parse(text)
        layout = l
        var t = activeTab()
        var panes = t ? t.panes.map(function (p) { return p.id }) : []
        var dividers = t ? t.dividers.map(function (d) { return d.tab + ":" + d.path }) : []
        var tabs = l.tabs.map(function (x) { return x.id })
        if (!same(panes, paneIds)) paneIds = panes
        if (!same(dividers, dividerKeys)) dividerKeys = dividers
        if (!same(tabs, tabIds)) tabIds = tabs
    }

    // Puts the keyboard on the focused pane.
    function focusPane() {
        for (var i = 0; i < panes.count; ++i) {
            var it = panes.itemAt(i)
            if (it && it.isFocused) it.gridItem.forceActiveFocus()
        }
    }

    Connections {
        target: ws.ctl
        function onLayoutChanged() { ws.reload() }
        function onLookChanged() {
            ws.bg = ws.hex(ws.ctl.setting("background"))
            ws.fg = ws.hex(ws.ctl.setting("foreground"))
            ws.accent = ws.hex(ws.ctl.setting("accent"))
        }
    }
    Component.onCompleted: reload()

    // ---- the tab bar ---------------------------------------------------------------------------
    Item {
        id: bar
        height: ws.barH
        width: parent.width
        anchors.top: ws.tabsBottom ? undefined : parent.top
        anchors.bottom: ws.tabsBottom ? parent.bottom : undefined

        Row {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4

            Repeater {
                model: ws.tabIds
                delegate: Rectangle {
                    id: pill
                    required property int modelData
                    required property int index
                    readonly property var tab: ws.tabInfo(modelData)
                    readonly property bool active: tab !== null && tab.active
                    height: 26
                    width: Math.min(180, Math.max(90, label.implicitWidth + 44))
                    radius: 8
                    color: active ? ws.accent : (hover.hovered ? Qt.rgba(ws.fg.r, ws.fg.g, ws.fg.b, 0.2) : ws.dim)

                    HoverHandler { id: hover }
                    Text {
                        id: label
                        anchors.left: parent.left
                        anchors.leftMargin: 10
                        anchors.right: close.left
                        anchors.verticalCenter: parent.verticalCenter
                        elide: Text.ElideRight
                        color: pill.active ? ws.bg : ws.fg
                        font.pixelSize: 13
                        text: (pill.index + 1) + "  " + (pill.tab ? pill.tab.name : "")
                    }
                    MouseArea {
                        anchors.fill: parent
                        acceptedButtons: Qt.LeftButton | Qt.MiddleButton
                        onClicked: function (m) {
                            if (m.button === Qt.MiddleButton) ws.ctl.closeTab(pill.modelData)
                            else ws.ctl.focusTab(pill.modelData)
                        }
                    }
                    // The only close control: shown on hover and on the active tab.
                    Text {
                        id: close
                        anchors.right: parent.right
                        anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        visible: pill.active || hover.hovered
                        color: pill.active ? ws.bg : ws.fg
                        font.pixelSize: 15
                        text: "×"
                        MouseArea {
                            anchors.fill: parent
                            anchors.margins: -5
                            onClicked: ws.ctl.closeTab(pill.modelData)
                        }
                    }
                }
            }

            Repeater {
                model: [
                    { kind: "plus", act: "new_tab" },
                    { kind: "side", act: "split_side" },
                    { kind: "stacked", act: "split_stacked" }
                ]
                delegate: Rectangle {
                    id: btn
                    required property var modelData
                    width: 26
                    height: 26
                    radius: 8
                    color: bh.hovered ? Qt.rgba(ws.fg.r, ws.fg.g, ws.fg.b, 0.2) : "transparent"
                    HoverHandler { id: bh }
                    // Drawn, not typeset: no font has to carry the icons.
                    Rectangle {
                        visible: btn.modelData.kind !== "plus"
                        anchors.centerIn: parent
                        width: 14
                        height: 12
                        radius: 2
                        color: "transparent"
                        border.width: 1
                        border.color: ws.fg
                        Rectangle {
                            color: ws.fg
                            x: btn.modelData.kind === "side" ? 6 : 0
                            y: btn.modelData.kind === "side" ? 0 : 5
                            width: btn.modelData.kind === "side" ? 1 : parent.width - 2
                            height: btn.modelData.kind === "side" ? parent.height - 2 : 1
                        }
                    }
                    Text {
                        visible: btn.modelData.kind === "plus"
                        anchors.centerIn: parent
                        color: ws.fg
                        font.pixelSize: 18
                        text: "+"
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            ws.ctl.action(btn.modelData.act)
                            ws.focusPane()
                        }
                    }
                }
            }
        }
    }

    // ---- the panes -----------------------------------------------------------------------------
    Item {
        id: area
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: ws.tabsBottom ? parent.top : bar.bottom
        anchors.bottom: ws.tabsBottom ? bar.top : parent.bottom
        anchors.topMargin: ws.tabsBottom ? 0 : 4
        anchors.bottomMargin: ws.tabsBottom ? 4 : 0

        Repeater {
            id: panes
            model: ws.paneIds
            delegate: Item {
                id: cell
                required property int modelData
                readonly property var info: ws.paneInfo(modelData)
                readonly property bool isFocused: ws.layout.focused === modelData
                property alias gridItem: grid
                readonly property bool multi: ws.paneIds.length > 1
                visible: info !== null
                x: info ? Math.round(info.x * area.width) : 0
                y: info ? Math.round(info.y * area.height) : 0
                width: info ? Math.round((info.x + info.w) * area.width) - x : 0
                height: info ? Math.round((info.y + info.h) * area.height) - y : 0

                Rectangle {
                    anchors.fill: parent
                    anchors.margins: 1
                    color: "transparent"
                    radius: 4
                    border.width: cell.multi && cell.isFocused ? 1 : 0
                    border.color: ws.accent
                }
                GridView {
                    id: grid
                    pane: cell.modelData
                    anchors.fill: parent
                    anchors.margins: cell.multi ? 4 : 0
                    focus: cell.isFocused
                    onActiveFocusChanged: if (activeFocus && !cell.isFocused) ws.ctl.focusPane(cell.modelData)
                    Component.onCompleted: if (cell.isFocused) forceActiveFocus()
                }
                // A click focuses the pane; the grid still gets it.
                MouseArea {
                    anchors.fill: parent
                    acceptedButtons: Qt.LeftButton
                    onPressed: function (m) {
                        ws.ctl.focusPane(cell.modelData)
                        grid.forceActiveFocus()
                        m.accepted = false
                    }
                }
                // A pane that ended while held open, or never started.
                Rectangle {
                    visible: cell.info !== null && cell.info.state !== "running"
                    anchors.centerIn: parent
                    color: ws.bg
                    border.color: ws.dim
                    radius: 6
                    width: Math.min(parent.width - 16, note.implicitWidth + 24)
                    height: note.implicitHeight + 16
                    Text {
                        id: note
                        anchors.centerIn: parent
                        width: parent.width - 24
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.Wrap
                        color: ws.fg
                        font.pixelSize: 13
                        text: !cell.info ? "" : cell.info.state === "exited"
                              ? "Exited with status " + cell.info.status + " (click to close)"
                              : cell.info.state === "failed" ? "Could not start: " + cell.info.message
                              : "Session ended (click to close)"
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: ws.ctl.dismiss(cell.modelData)
                    }
                }
            }
        }

        // Dividers: drag to resize.
        Repeater {
            model: ws.dividerKeys
            delegate: Item {
                id: div
                required property string modelData
                readonly property var info: ws.dividerInfo(modelData)
                readonly property bool side: info !== null && info.axis === "side"
                visible: info !== null
                x: !info ? 0 : side ? Math.round((info.x + info.w * info.ratio) * area.width) - 3 : Math.round(info.x * area.width)
                y: !info ? 0 : side ? Math.round(info.y * area.height) : Math.round((info.y + info.h * info.ratio) * area.height) - 3
                width: !info ? 0 : side ? 6 : Math.round(info.w * area.width)
                height: !info ? 0 : side ? Math.round(info.h * area.height) : 6
                z: 2
                MouseArea {
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: div.side ? Qt.SplitHCursor : Qt.SplitVCursor
                    onPositionChanged: function (m) {
                        if (!pressed || !div.info) return
                        var p = mapToItem(area, m.x, m.y)
                        var d = div.info
                        var r = div.side ? (p.x / area.width - d.x) / d.w : (p.y / area.height - d.y) / d.h
                        ws.ctl.setRatio(d.tab, d.path, r)
                    }
                }
            }
        }
    }

    Text {
        anchors.centerIn: parent
        visible: ws.layout.notice.length > 0
        color: "#fb4934"
        text: ws.layout.notice
        font.pixelSize: 16
    }
}
