// Closing something that is still running asks first (part of Workspace.qml).
import QtQuick
import org.kyko.app

Rectangle {
    id: confirm

    // The workspace (Workspace.qml): its layout, colours, controller and helpers.
    property var host

    readonly property bool asking: confirm.host.layout.confirm !== null && confirm.host.layout.confirm !== undefined
    visible: asking
    z: 10
    anchors.centerIn: parent
    width: Math.min(parent.width - 32, 420)
    height: ask.implicitHeight + 32
    radius: confirm.host.radius
    color: confirm.host.bg
    border.width: 1
    border.color: confirm.host.accent
    focus: asking
    onAskingChanged: if (asking) forceActiveFocus(); else confirm.host.focusPane()
    Keys.onReturnPressed: confirm.host.ctl.confirm(true)
    Keys.onEnterPressed: confirm.host.ctl.confirm(true)
    Keys.onEscapePressed: confirm.host.ctl.confirm(false)
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
            color: confirm.host.fg
            font.pixelSize: 14
            text: !confirm.asking ? ""
                  : confirm.host.layout.confirm.what === "paste"
                    ? "Paste " + confirm.host.layout.confirm.programs + " lines? The program is not ready for a paste, so each line runs as it arrives."
                    : "Close this " + confirm.host.layout.confirm.what + "? " + confirm.host.layout.confirm.programs + " is still running in it."
        }
        Row {
            spacing: 8
            anchors.right: parent.right
            Repeater {
                model: confirm.asking && confirm.host.layout.confirm.what === "paste"
                       ? [{ label: "Cancel", yes: false }, { label: "Paste", yes: true }]
                       : [{ label: "Keep it", yes: false }, { label: "Close", yes: true }]
                delegate: Rectangle {
                    required property var modelData
                    width: 90
                    height: 28
                    radius: confirm.host.r(0.8)
                    color: modelData.yes ? confirm.host.accent : confirm.host.dim
                    Text {
                        anchors.centerIn: parent
                        color: parent.modelData.yes ? confirm.host.bg : confirm.host.fg
                        font.pixelSize: 13
                        text: parent.modelData.label
                    }
                    MouseArea { anchors.fill: parent; onClicked: confirm.host.ctl.confirm(parent.modelData.yes) }
                }
            }
        }
    }
}
