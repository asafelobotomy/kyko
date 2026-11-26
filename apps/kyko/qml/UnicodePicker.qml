// Ctrl+Shift+U: a character by its code point or name, typed into the focused pane (part of
// Workspace.qml).
import QtQuick
import org.kyko.app

Rectangle {
    id: picker

    // The workspace (Workspace.qml): its layout, colours, controller and helpers.
    property var host

    readonly property bool open: picker.host.layout.unicode === true
    property var found: []
    property int current: 0
    visible: open
    z: 9
    anchors.topMargin: 24
    height: pickerColumn.implicitHeight + 16
    radius: picker.host.radius
    color: picker.host.bg
    border.width: 1
    border.color: picker.host.accent
    onOpenChanged: {
        if (open) {
            pickerField.text = ""
            found = []
            current = 0
            pickerField.forceActiveFocus()
        } else picker.host.focusPane()
    }
    function finish(typed) {
        var ch = typed && found.length > 0 ? found[current].ch : ""
        picker.host.ctl.unicodeDone(ch, typed && ch.length > 0)
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
            color: picker.host.fg
            font.pixelSize: 14
            clip: true
            Text {
                visible: pickerField.text.length === 0
                color: picker.host.fg
                opacity: 0.5
                font.pixelSize: 14
                text: "Code point (1f600) or name (smiling face)"
            }
            onTextEdited: {
                picker.found = JSON.parse(picker.host.ctl.unicodeSearch(text))
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
                radius: picker.host.r(0.6)
                color: index === picker.current ? picker.host.accent : "transparent"
                Row {
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.left: parent.left
                    anchors.leftMargin: 8
                    spacing: 12
                    Text {
                        width: 28
                        horizontalAlignment: Text.AlignHCenter
                        color: index === picker.current ? picker.host.bg : picker.host.fg
                        font.pixelSize: 18
                        text: modelData.ch
                    }
                    Text {
                        width: 72
                        color: index === picker.current ? picker.host.bg : picker.host.fg
                        opacity: 0.8
                        font.pixelSize: 12
                        font.family: "monospace"
                        anchors.verticalCenter: parent.verticalCenter
                        text: modelData.code
                    }
                    Text {
                        color: index === picker.current ? picker.host.bg : picker.host.fg
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
            color: picker.host.fg
            opacity: 0.6
            font.pixelSize: 12
            text: "Nothing found"
        }
    }
}
