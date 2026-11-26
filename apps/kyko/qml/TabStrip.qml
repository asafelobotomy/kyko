// The tab bar (part of Workspace.qml): one pill per tab, each renamed in place.
import QtQuick
import org.kyko.app

Item {
    id: bar

    // The workspace (Workspace.qml): its layout, colours, controller and helpers.
    property var host

    height: bar.host.barH
    width: parent.width
    anchors.top: bar.host.tabsBottom ? undefined : parent.top
    anchors.bottom: bar.host.tabsBottom ? parent.bottom : undefined

    Row {
        anchors.verticalCenter: parent.verticalCenter
        spacing: 4

        Repeater {
            model: bar.host.tabIds
            delegate: Rectangle {
                id: pill
                required property int modelData
                required property int index
                readonly property var tab: bar.host.tabInfo(modelData)
                readonly property bool active: tab !== null && tab.active
                height: 26
                width: Math.min(180, Math.max(90, label.implicitWidth + 44))
                radius: bar.host.r(0.8)
                color: active ? bar.host.accent : (hover.hovered ? Qt.rgba(bar.host.fg.r, bar.host.fg.g, bar.host.fg.b, 0.2) : bar.host.dim)

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
                    color: pill.active ? bar.host.bg : bar.host.fg
                    selectionColor: bar.host.bg
                    selectedTextColor: bar.host.fg
                    font.pixelSize: 13
                    clip: true
                    onAccepted: {
                        bar.host.ctl.renameTab(pill.modelData, text)
                        pill.editing = false
                        bar.host.focusPane()
                    }
                    Keys.onEscapePressed: { pill.editing = false; bar.host.focusPane() }
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
                    color: pill.active ? bar.host.bg : bar.host.fg
                    font.pixelSize: 13
                    text: (pill.index + 1) + "  " + (pill.tab ? pill.tab.name : "")
                }
                MouseArea {
                    anchors.fill: parent
                    acceptedButtons: Qt.LeftButton | Qt.MiddleButton
                    onClicked: function (m) {
                        if (m.button === Qt.MiddleButton) bar.host.ctl.closeTab(pill.modelData)
                        else bar.host.ctl.focusTab(pill.modelData)
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
                    color: pill.active ? bar.host.bg : bar.host.fg
                    font.pixelSize: 15
                    text: "×"
                    MouseArea {
                        anchors.fill: parent
                        anchors.margins: -5
                        onClicked: bar.host.ctl.closeTab(pill.modelData)
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
                radius: bar.host.r(0.8)
                color: bh.hovered ? Qt.rgba(bar.host.fg.r, bar.host.fg.g, bar.host.fg.b, 0.2) : "transparent"
                HoverHandler { id: bh }
                // Drawn, not typeset: no font has to carry the icons.
                Rectangle {
                    visible: btn.modelData.kind === "side" || btn.modelData.kind === "stacked"
                    anchors.centerIn: parent
                    width: 14
                    height: 12
                    radius: bar.host.r(0.2)
                    color: "transparent"
                    border.width: 1
                    border.color: bar.host.fg
                    Rectangle {
                        color: bar.host.fg
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
                    radius: width / 2
                    color: "transparent"
                    border.width: 2
                    border.color: bar.host.fg
                    Rectangle {
                        anchors.centerIn: parent
                        width: 4
                        height: 4
                        radius: width / 2
                        color: bar.host.fg
                    }
                }
                Text {
                    visible: btn.modelData.kind === "plus"
                    anchors.centerIn: parent
                    color: bar.host.fg
                    font.pixelSize: 18
                    text: "+"
                }
                MouseArea {
                    anchors.fill: parent
                    onClicked: {
                        bar.host.ctl.action(btn.modelData.act)
                        bar.host.focusPane()
                    }
                }
            }
        }
    }
}
