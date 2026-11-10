import QtQuick
import QtQuick.Window
import org.kde.layershell as LayerShell
import org.kyko.spike
import org.kyko.spike.native

Window {
    id: root
    property bool shown: true
    readonly property int gap: 12
    readonly property int panelH: Math.round(Screen.height * 0.45)

    width: Math.round(Screen.width * 0.88)
    height: panelH + gap
    visible: true
    color: "transparent"
    flags: Qt.FramelessWindowHint

    LayerShell.Window.layer: toggler.layer_top() ? LayerShell.Window.LayerTop : LayerShell.Window.LayerOverlay
    LayerShell.Window.anchors: LayerShell.Window.AnchorTop
    LayerShell.Window.keyboardInteractivity: toggler.no_focus() ? LayerShell.Window.KeyboardInteractivityNone : LayerShell.Window.KeyboardInteractivityOnDemand
    LayerShell.Window.scope: "kyko"
    LayerShell.Window.exclusionZone: -1

    onShownChanged: {
        if (shown) {
            visible = true
            if (!toggler.no_focus()) {
                requestActivate()
                grid.forceActiveFocus()
            }
        }
        slide.to = shown ? gap : -panelH
        slide.restart()
    }

    NumberAnimation {
        id: slide
        target: panel
        property: "y"
        duration: 150
        easing.type: Easing.OutCubic
        onFinished: if (!root.shown) root.visible = false
    }

    Rectangle {
        id: panel
        y: root.gap
        width: parent.width
        height: root.panelH
        radius: 10
        color: "#282828"
        border.width: 2
        border.color: "#d79921"

        GridItem {
            id: grid
            anchors.fill: parent
            anchors.margins: 8 + panel.border.width
            focus: true
        }

        Text {
            id: counter
            property int n: 0
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: 10
            color: "#b0a589"
            font.pixelSize: 14
            text: grid.cols + "×" + grid.rows + " · state " + n + (root.active ? " · focused" : " · not focused")
            Timer { interval: 1000; running: true; repeat: true; onTriggered: counter.n++ }
        }
    }

    Toggler {
        id: toggler
        Component.onCompleted: {
            start()
            if (start_hidden())
                root.shown = false
        }
        onToggled: root.shown = !root.shown
    }
}
