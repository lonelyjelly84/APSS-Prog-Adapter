# Psat PSU PCB v2
The PSU PCB provides 1.8V, 3.3V, 5V rails and a raw battery voltage rail. The various supply rails can be always on, or controlled by software (see Functionality). USB-C and micro-USB connectors are present to charge the battery using a standard USB wall charger.
The supply rail voltages can be adjusted by changing the feedback resistor values. See the schematic for details.

## Functionality
### Controlling the Supply Rails
The PSU board offers two different control modes for each supply rail, controlled by the switches the S1 component: 
In the “ON” mode the relevant supply is always on. This is the simplest operating mode and no interaction with the enable pins are necessary. In this mode the enable pin is connected to Vraw via a 1.5k current-limiting resistor.

In the “OFF / SW control” mode, the supply defaults to off (pulled down by a 1Megohm resistor) but a microcontroller can enable the supply by pulling the enable pin high. If you want to programmatically enable a supply during operation you need to use this mode and connect your microcontroller to the enable pins on the header. See ‘Connections’ for the exact voltage(s) required to enable each supply.


*Note: If you configure S1 into the “ON” mode and have also connected the enable pin to a microcontroller, make sure to leave that GPIO pin as an input pin, as otherwise you will waste power through the 1.5k resistor.*

### USB charging
The board is capable of charging a lithium-ion battery by connecting a USB charger.
The two USB connectors (USB-C and micro-USB) are provided for convenience. DO NOT CHARGE USING BOTH CONNECTORS AT ONCE.
The battery will charge up to 800mA (USB power supply willing). This rate can be decreased by changing the value of R6, according to the datasheet of the battery charger.

### Disable while charging
The final S1 switch (labelled '*' on the PCB) is the 'supply disabled while charging' switch. It determines whether all the supply rails (including Vraw) are available while a USB cable is plugged in. The battery will of course charge faster if the load is disabled during charging. The BQ21040 battery charger IC states that it can also power loads during charging, so long as the charging time does not exceed 10 hours as a result, but this is not universally true for all charging ICs. When this switch is set to "ON" the load is disabled during charging. When it is set to "SW control" the supplies are enabled even while plugged in.

### 'Remove Before Flight’ Connector
RBF is a connector that controls whether the battery is connected to the PSU and stack. When a jumper is placed across RBF (shorting the two pins) the battery is disconnected from the PSU and hence the stack. When the jumper is removed the entire stack is powered. It is placed at a right-angle so that the jumper may be removed prior to launch through a hole in the exterior PSat shell.

## Connections
Header H1:

 - 1V8 -  1.8V step-down supply rail, capable of 2A output, battery not withstanding.
 - 3V3 -  3.3V step-down supply rail, capable of 2A output, battery not withstanding. *
 - 5V  -  5V step-up supply rail, capable of 2A output, battery not withstanding.
 - GND -  Common ground for all supply rails.
 - Vraw - Raw battery voltage. For a Li-ion battery this is between ~4.2V and ~3V.
 - 1V8_PG - Power good indicator for the 1.8V rail. Open-drain output: While the output voltage is more than 20% away from the regulation level this pin is pulled low, otherwise it is floating - you should attach this to a voltage source via a pullup resistor of your choosing.
 - 3V3_PG - Power good indicator for the 3.3V rail. Open-drain output: While the output voltage is more than 20% away from the regulation level this pin is pulled low,  otherwise it is floating - you should attach this to a voltage source via a pullup resistor of your choosing.
 - EN_1V8 - This pin may be pulled up above 1.5V to enable the 1.8V supply. The pin has a 1 Megohm pulldown resistor attached. When the equivalent S1 switch is in the "ON" position a 1.5k pullup resistor pulls this pin up to Vraw.
 - EN_3V3 - This pin may be pulled up above 1.5V to enable the 3.3V supply. The pin has a 1 Megohm pulldown resistor attached. When the equivalent S1 switch is in the "ON" position a 1.5k pullup resistor pulls this pin up to Vraw.
 - EN_5V - This pin may be pulled up above 1.2V to enable the 5V supply. The pin has a 1 Megohm pulldown resistor attached. When the equivalent S1 switch is in the "ON" position a 1.5k pullup resistor pulls this pin up to Vraw.

 \* *Note: This voltage is only stepped-down from the battery voltage, so if that goes below 3.3V this supply rail will as well. This means that across a full battery discharge this rail is nominally between 3.3V - 3.0V.*

## PCB Stacking
The PSat PSU is designed to be used in conjunction with either a single payload PCB (i.e. ‘2 PCB stack’) or in a stack of multiple PCBs (‘Multi-PCB stack’).

There are three types of 2.54mm pitch headers available in the lab, and which the PCB was designed with in mind:
Male headers with a 6mm mating length, 2.54mm insulation and a 3mm post.
Female headers with 8.5mm insulation and a 3mm post.
'Arduino female stacking headers' with 8.5mm of insulation, and 10.5mm posts.
### 2 PCB Stack
The simplest stack uses the supplied female header on the payload PSU and regular male headers on the payload board, pointed down. This produces a stack height of about 5mm.

The PSU is designed to go at the bottom of the stack (hence why the example payload has a cutout for the battery), but it can be moved up in the stack provided the battery can fit in the remaining space.

If a larger stack height is needed the standard female headers can be used to grant either ~8.5mm or ~11mm stack height (depending on whether the male pins are soldered with the plastic insulation between the PCBs or on the other side).

The stack height can be eliminated entirely if the male header is soldered such that the insulation is not between the two PCBs. Note that this would require the PSU PCB to be flipped (reversing the order of the header H1), as the underside of the PSU PCB must be used to mate against the payload (as no components on this side to interfere).

### Multi-PCB stack
The simplest stack uses the female stacking headers for each layer, except for the bottom which can use a standard female header, and the top which can use a standard male header (pointing down). This enforces a board-to-board stack height of 11mm between payload boards, and a stack height of around 5mm between the PSU board and the first payload board.

## Testing
To verify PCB functionality after assembly (or a mishap):

1. To verify the regulators work as expected, use a benchtop supply to generate 4V with a 100mA current limit. Connect the negative end to GND on the main header, and the positive lead to Vraw. When the supply is enabled (either by the switch, solder bridges, or enable pin depending on configuration), each of the supplies should produce an output voltage within ~5% of the rated voltage. Transient current of the PSU board should be very low, nominally less than or equal to 1mA. 

    *Troubleshooting:* If one of the supplies does not work, check whether the enable pin for that supply is high. If the enable pin is not high, double-check the switch or solder bridge configuration. If the enable pin is high but the supply is not outputting the correct voltage look for bad solder connections around the related components. If you see an output voltage of 0.6V on the 3V3 or 1V8 supply this indicates a problem with the feedback resistor divider, reflow the soldering and add solder to any joints that look dry. If 0V is seen on the supply output, verify the soldering of the regulator IC and the inductor. 

2. To verify the battery charging circuit, first connect a benchtop supply set to 5V (100mA current limit), connect the negative lead to the ground pin on the output header and the positive end to the exposed test pad between the USB connectors and the battery charger IC (On v2.0.6 this is NetC6_2 but on future revisions may be labelled as Vbus). Then probe from GND to the exposed pad between the battery charger and the battery connector (Vbat). It should read about 4V.

    *Troubleshooting:* If the output voltage is not approximately 4V check the soldering on the battery charger IC and related components.

3. To test the RBF header set up a benchtop supply with a 4V and 100mA current limit. Connect the RBF header. Connect the negative end to GND and the positive pin to the positive end of the battery connector using a female jumper wire (this connection is tenuous and may require you to hold to the jumper wire in place). Verify that the 1V8, 3V3, 5V and Vraw outputs show near 0V when probed. Remove the RBF header and check the outputs again. Vraw should always show 4V, but the other supplies may or may not be enabled based on the switch/jumper configuration.

    *Troubleshooting:* If Vraw does not show 4V when the RBF is removed, check the soldering of the cutoff transistor (on v2.0.6 this is Q1) and it's pullup resistor (v2.0.6: R11)

4. To test battery charging, first **verify that the black battery wire would connect to the side of the battery connector labelled with '-ve'**, equivalently the red wire should plug into the pin closest to the '+' mark near the battery connector. Next, plug in the battery. **Feel the PCB for any spots that get noticeably hot, and if so immediately remove the battery** and double-check the battery polarity and header soldering orientation. Using a benchtop supply set to 5V (1A limit) and connect the negative lead to GND and the positive lead to the exposed test point between the USB and the battery charger IC (same as test 2). Monitor the current drawn by the PCB - it should at no point exceed 800mA. The battery charging LED should illuminate if the battery is not already fully charged. If everything appears normal, you may now charge the battery via the USB connectors.

    *Troubleshooting:* If the board draws a normal amount of current for charging but the LED does not light up, remove the battery and connections. Check the orientation of the diode using the diode mode on a multimeter. The positive lead should connect to the side of the diode connected to the nearby resistor, and the negative lead on the other side. If the diode does not noticeably light up (albeit faintly), reverse the orientation of the multimeter leads. If the LED now lights up it is on backwards.

## Future changes
Maybe add a resettable fuse between the two USB connector’s bus voltages?

Maybe add a polyfuse to battery and each rail’s outputs

## Version History + Changelog
Version 2:
Move away from a centralised, fixed-size stack header to a flexible ‘only-what-is-necessary’ header: Rather than using a single 30-pin header with many unused pins, we use only a 10-pin header because the PSU PCB only needs 10 pins. Removed unused header pins as a result. PCBs further above in the stack can add additional headers adjacent to ours if necessary. 
Also switched to 2.54mm header due to a lack of 1.27mm ‘stacking’ headers. 2.54mm headers are also easier to route around, which actually makes it easier to add more pins to other PCBs in the stack if necessary, even though they’re bigger. 
Fixed Q1 by swapping pins 2 and 3. The mosfet body diode allowed Vbat to power the load even when the mosfet was (supposed to be) acting as an open circuit.
Added a resistor between Vraw and the EN_XYZ pins to prevent possible short circuit with microcontroller if switch is improperly configured
