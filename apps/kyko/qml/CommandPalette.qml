// Ctrl+Shift+P: every action, tab and recent command, found by typing (part of Workspace.qml).
import QtQuick
import org.kyko.app

Rectangle {
    id: palette

    // The workspace (Workspace.qml): its layout, colours, controller and helpers.
    property var host

    readonly property bool open: palette.host.layout.palette === true
    property var found: []
    property int current: 0
    visible: open
    z: 9
    anchors.topMargin: 24
    height: paletteColumn.implicitHeight + 16
    radius: palette.host.radius
    color: palette.host.bg
    border.width: 1
    border.color: palette.host.accent
    onOpenChanged: {
        if (open) {
            paletteField.text = ""
            refresh()
            paletteField.forceActiveFocus()
        } else palette.host.focusPane()
    }
    function refresh() {
        found = JSON.parse(palette.host.ctl.paletteItems(paletteField.text))
        current = 0
    }
    function finish(chosen) {
        palette.host.ctl.paletteDone(chosen && found.length > 0 ? found[current].id : "")
    }
    MouseArea { anchors.fill: parent }
    Column {
        id: paletteColumn
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 8
        spacing: 2
        TextInput {
            id: paletteField
            width: parent.width
            color: palette.host.fg
            font.pixelSize: 14
            clip: true
            Text {
                visible: paletteField.text.length === 0
                color: palette.host.fg
                opacity: 0.5
                font.pixelSize: 14
                text: "Type an action, a tab or a command you ran"
            }
            onTextEdited: palette.refresh()
            Keys.onReturnPressed: palette.finish(true)
            Keys.onEnterPressed: palette.finish(true)
            Keys.onEscapePressed: palette.finish(false)
            Keys.onUpPressed: palette.current = Math.max(0, palette.current - 1)
            Keys.onDownPressed: palette.current = Math.min(palette.found.length - 1, palette.current + 1)
        }
        Item { width: 1; height: 4 }
        Repeater {
            model: palette.found.slice(0, 12)
            Rectangle {
                required property var modelData
                required property int index
                readonly property bool picked: index === palette.current
                width: paletteColumn.width
                height: 28
                radius: palette.host.r(0.6)
                color: picked ? palette.host.accent : "transparent"
                Text {
                    anchors.left: parent.left
                    anchors.leftMargin: 8
                    anchors.right: hint.left
                    anchors.rightMargin: 12
                    anchors.verticalCenter: parent.verticalCenter
                    elide: Text.ElideRight
                    color: parent.picked ? palette.host.bg : palette.host.fg
                    font.pixelSize: 13
                    font.family: parent.modelData.kind === "command" ? "monospace" : ""
                    text: parent.modelData.label
                }
                Text {
                    id: hint
                    anchors.right: parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    color: parent.picked ? palette.host.bg : palette.host.fg
                    opacity: 0.7
                    font.pixelSize: 12
                    text: parent.modelData.hint
                }
                MouseArea {
                    anchors.fill: parent
                    onClicked: { palette.current = parent.index; palette.finish(true) }
                }
            }
        }
        Text {
            visible: palette.found.length === 0
            color: palette.host.fg
            opacity: 0.6
            font.pixelSize: 12
            text: "Nothing found"
        }
    }
}
