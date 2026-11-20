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

                    property bool editing: false
                    function edit() {
                        editing = true
                        field.text = tab ? tab.name : ""
                        field.forceActiveFocus()
                        field.selectAll()
                    }
                    HoverHandler { id: hover }
                    // A rename sticks until it is cleared: an empty name takes the automatic one back.
                    TextInput {
                        id: field
                        visible: pill.editing
                        anchors.left: parent.left
                        anchors.leftMargin: 10
                        anchors.right: close.left
                        anchors.verticalCenter: parent.verticalCenter
                        color: pill.active ? ws.bg : ws.fg
                        selectionColor: ws.bg
                        selectedTextColor: ws.fg
                        font.pixelSize: 13
                        clip: true
                        onAccepted: {
                            ws.ctl.renameTab(pill.modelData, text)
                            pill.editing = false
                            ws.focusPane()
                        }
                        Keys.onEscapePressed: { pill.editing = false; ws.focusPane() }
                        onActiveFocusChanged: if (!activeFocus && pill.editing) pill.editing = false
                    }
                    Text {
                        id: label
                        visible: !pill.editing
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
                        onDoubleClicked: function (m) {
                            if (m.button === Qt.LeftButton) pill.edit()
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
                    { kind: "stacked", act: "split_stacked" },
                    { kind: "gear", act: "preferences" }
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
                        visible: btn.modelData.kind === "side" || btn.modelData.kind === "stacked"
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
                    // The settings button: a ring with a hub.
                    Rectangle {
                        visible: btn.modelData.kind === "gear"
                        anchors.centerIn: parent
                        width: 14
                        height: 14
                        radius: 7
                        color: "transparent"
                        border.width: 2
                        border.color: ws.fg
                        Rectangle {
                            anchors.centerIn: parent
                            width: 4
                            height: 4
                            radius: 2
                            color: ws.fg
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
                // A zoomed tab shows its one pane as if it were alone.
                readonly property bool multi: ws.paneIds.length > 1 && !(ws.activeTab() && ws.activeTab().zoomed)
                // Typing reaches every pane of the tab: each one wears a red border.
                readonly property bool broadcasting: ws.activeTab() !== null && ws.activeTab().broadcast === true
                visible: info !== null && !info.hidden
                x: info ? Math.round(info.x * area.width) : 0
                y: info ? Math.round(info.y * area.height) : 0
                width: info ? Math.round((info.x + info.w) * area.width) - x : 0
                height: info ? Math.round((info.y + info.h) * area.height) - y : 0

                Rectangle {
                    anchors.fill: parent
                    anchors.margins: 1
                    color: "transparent"
                    radius: 4
                    border.width: cell.multi && (cell.isFocused || cell.broadcasting) ? (cell.broadcasting ? 2 : 1) : 0
                    border.color: cell.broadcasting ? "#e5534b" : ws.accent
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

    // Search in the focused pane's scrollback: the match is selected and brought into view.
    Rectangle {
        id: findBar
        readonly property bool open: ws.layout.find !== undefined && ws.layout.find.open
        visible: open
        z: 9
        anchors.right: area.right
        anchors.top: area.top
        anchors.rightMargin: 12
        anchors.topMargin: 8
        width: 300
        height: 32
        radius: 8
        color: ws.bg
        border.width: 1
        border.color: ws.accent
        onOpenChanged: {
            if (open) { findField.forceActiveFocus(); findField.selectAll() }
            else ws.focusPane()
        }
        MouseArea { anchors.fill: parent }
        TextInput {
            id: findField
            anchors.left: parent.left
            anchors.leftMargin: 10
            anchors.right: count.left
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            color: ws.fg
            font.pixelSize: 13
            clip: true
            onTextEdited: ws.ctl.search(0, text)
            Keys.onReturnPressed: function (e) { ws.ctl.search((e.modifiers & Qt.ShiftModifier) ? 2 : 1, "") }
            Keys.onEnterPressed: function (e) { ws.ctl.search((e.modifiers & Qt.ShiftModifier) ? 2 : 1, "") }
            Keys.onEscapePressed: ws.ctl.search(3, "")
            Keys.onUpPressed: ws.ctl.search(2, "")
            Keys.onDownPressed: ws.ctl.search(1, "")
        }
        Text {
            id: count
            anchors.right: parent.right
            anchors.rightMargin: 10
            anchors.verticalCenter: parent.verticalCenter
            color: ws.fg
            opacity: 0.7
            font.pixelSize: 12
            text: !findBar.open ? "" : findField.text.length === 0 ? ""
                  : ws.layout.find.total === 0 ? "No matches"
                  : ws.layout.find.pos + " / " + ws.layout.find.total
        }
    }

    // Ctrl+Shift+U: a character by its code point or name, typed into the focused pane.
    Rectangle {
        id: picker
        readonly property bool open: ws.layout.unicode === true
        property var found: []
        property int current: 0
        visible: open
        z: 9
        anchors.horizontalCenter: area.horizontalCenter
        anchors.top: area.top
        anchors.topMargin: 24
        width: Math.min(area.width - 32, 460)
        height: pickerColumn.implicitHeight + 16
        radius: 10
        color: ws.bg
        border.width: 1
        border.color: ws.accent
        onOpenChanged: {
            if (open) {
                pickerField.text = ""
                found = []
                current = 0
                pickerField.forceActiveFocus()
            } else ws.focusPane()
        }
        function finish(typed) {
            var ch = typed && found.length > 0 ? found[current].ch : ""
            ws.ctl.unicodeDone(ch, typed && ch.length > 0)
        }
        MouseArea { anchors.fill: parent }
        Column {
            id: pickerColumn
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 8
            spacing: 4
            TextInput {
                id: pickerField
                width: parent.width
                color: ws.fg
                font.pixelSize: 14
                clip: true
                Text {
                    visible: pickerField.text.length === 0
                    color: ws.fg
                    opacity: 0.5
                    font.pixelSize: 14
                    text: "Code point (1f600) or name (smiling face)"
                }
                onTextEdited: {
                    picker.found = JSON.parse(ws.ctl.unicodeSearch(text))
                    picker.current = 0
                }
                Keys.onReturnPressed: picker.finish(true)
                Keys.onEnterPressed: picker.finish(true)
                Keys.onEscapePressed: picker.finish(false)
                Keys.onUpPressed: picker.current = Math.max(0, picker.current - 1)
                Keys.onDownPressed: picker.current = Math.min(picker.found.length - 1, picker.current + 1)
            }
            Repeater {
                model: picker.found
                Rectangle {
                    required property var modelData
                    required property int index
                    width: pickerColumn.width
                    height: 28
                    radius: 6
                    color: index === picker.current ? ws.accent : "transparent"
                    Row {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 8
                        spacing: 12
                        Text {
                            width: 28
                            horizontalAlignment: Text.AlignHCenter
                            color: index === picker.current ? ws.bg : ws.fg
                            font.pixelSize: 18
                            text: modelData.ch
                        }
                        Text {
                            width: 72
                            color: index === picker.current ? ws.bg : ws.fg
                            opacity: 0.8
                            font.pixelSize: 12
                            font.family: "monospace"
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.code
                        }
                        Text {
                            color: index === picker.current ? ws.bg : ws.fg
                            font.pixelSize: 13
                            anchors.verticalCenter: parent.verticalCenter
                            elide: Text.ElideRight
                            width: pickerColumn.width - 140
                            text: modelData.name
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: { picker.current = index; picker.finish(true) }
                    }
                }
            }
            Text {
                visible: pickerField.text.length > 0 && picker.found.length === 0
                color: ws.fg
                opacity: 0.6
                font.pixelSize: 12
                text: "Nothing found"
            }
        }
    }

    // Closing something that is still running asks first.
    Rectangle {
        id: confirm
        readonly property bool asking: ws.layout.confirm !== null && ws.layout.confirm !== undefined
        visible: asking
        z: 10
        anchors.centerIn: parent
        width: Math.min(parent.width - 32, 420)
        height: ask.implicitHeight + 32
        radius: 10
        color: ws.bg
        border.width: 1
        border.color: ws.accent
        focus: asking
        onAskingChanged: if (asking) forceActiveFocus(); else ws.focusPane()
        Keys.onReturnPressed: ws.ctl.confirm(true)
        Keys.onEnterPressed: ws.ctl.confirm(true)
        Keys.onEscapePressed: ws.ctl.confirm(false)
        // Swallows clicks meant for the panes behind.
        MouseArea { anchors.fill: parent }
        Column {
            id: ask
            anchors.centerIn: parent
            width: parent.width - 32
            spacing: 12
            Text {
                width: parent.width
                wrapMode: Text.Wrap
                color: ws.fg
                font.pixelSize: 14
                text: !confirm.asking ? ""
                      : ws.layout.confirm.what === "paste"
                        ? "Paste " + ws.layout.confirm.programs + " lines? The program is not ready for a paste, so each line runs as it arrives."
                        : "Close this " + ws.layout.confirm.what + "? " + ws.layout.confirm.programs + " is still running in it."
            }
            Row {
                spacing: 8
                anchors.right: parent.right
                Repeater {
                    model: confirm.asking && ws.layout.confirm.what === "paste"
                           ? [{ label: "Cancel", yes: false }, { label: "Paste", yes: true }]
                           : [{ label: "Keep it", yes: false }, { label: "Close", yes: true }]
                    delegate: Rectangle {
                        required property var modelData
                        width: 90
                        height: 28
                        radius: 8
                        color: modelData.yes ? ws.accent : ws.dim
                        Text {
                            anchors.centerIn: parent
                            color: parent.modelData.yes ? ws.bg : ws.fg
                            font.pixelSize: 13
                            text: parent.modelData.label
                        }
                        MouseArea { anchors.fill: parent; onClicked: ws.ctl.confirm(parent.modelData.yes) }
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
