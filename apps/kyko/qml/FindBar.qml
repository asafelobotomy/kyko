// Search in the focused pane's scrollback: the match is selected and brought into view (part of
// Workspace.qml).
import QtQuick
import org.kyko.app

Rectangle {
    id: findBar

    // The workspace (Workspace.qml): its layout, colours, controller and helpers.
    property var host

    readonly property bool open: findBar.host.layout.find !== undefined && findBar.host.layout.find.open
    visible: open
    z: 9
    anchors.rightMargin: 12
    anchors.topMargin: 8
    width: 300
    height: 32
    radius: findBar.host.r(0.8)
    color: findBar.host.bg
    border.width: 1
    border.color: findBar.host.accent
    onOpenChanged: {
        if (open) { findField.forceActiveFocus(); findField.selectAll() }
        else findBar.host.focusPane()
    }
    MouseArea { anchors.fill: parent }
    TextInput {
        id: findField
        anchors.left: parent.left
        anchors.leftMargin: 10
        anchors.right: count.left
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        color: findBar.host.fg
        font.pixelSize: 13
        clip: true
        onTextEdited: findBar.host.ctl.search(0, text)
        Keys.onReturnPressed: function (e) { findBar.host.ctl.search((e.modifiers & Qt.ShiftModifier) ? 2 : 1, "") }
        Keys.onEnterPressed: function (e) { findBar.host.ctl.search((e.modifiers & Qt.ShiftModifier) ? 2 : 1, "") }
        Keys.onEscapePressed: findBar.host.ctl.search(3, "")
        Keys.onUpPressed: findBar.host.ctl.search(2, "")
        Keys.onDownPressed: findBar.host.ctl.search(1, "")
    }
    Text {
        id: count
        anchors.right: parent.right
        anchors.rightMargin: 10
        anchors.verticalCenter: parent.verticalCenter
        color: findBar.host.fg
        opacity: 0.7
        font.pixelSize: 12
        text: !findBar.open ? "" : findField.text.length === 0 ? ""
              : findBar.host.layout.find.total === 0 ? "No matches"
              : findBar.host.layout.find.pos + " / " + findBar.host.layout.find.total
    }
}
