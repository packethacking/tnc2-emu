#include <stdio.h>
#include "patch.h"
#include "patchdef.h"

static char *openingtext[] = {
	"Select a menu from below, or hit escape to exit",
	"    Level 1 parameters", 
	"    Level 2 parameters", 
	"    Level 3 parameters", 
	"    Level 4 parameters", 
	"    Level 7 parameters", 
	"    IP Router Parameters",
	"    Miscellaneous Parameters",
	"    Meter Control Parameters",
	"    Save changes to image on disk",
	"    Store parameters in a file",
	"    Load parameters from a file",
	NULL
};

static char *level1text[] = 
{	"Select a parameter from below or hit Escape for previous menu",
	"    Persistance ( n / 256 )",
	"    Slot time ( 10's of milliseconds )",
	"    Full duplex",
	"    Send flags if no data ( full duplex )",
	"    Transmit keyup delay ( 10's of msec )",
	"    CWID repeat period ( seconds )",
	"    CWID keyer speed ( dot speed 10's msec )",
	"    RS232 host mode ( normal / DCD mode )",
	"    RS232 crosslink protocol", 0 
};

static char *level2text[] = 
{	"Select a parameter from below or hit Escape for previous menu",
	"    Node callsign",
	"    FRACK ( T1 ) time ( seconds )",
	"    AX.25 window size ( packets )",
	"    AX.25 retries",
	"    Ack ( T2 ) time ( 10's of milliseconds )",
	"    Active check ( T3 ) time ( 10's msecs )",
	"    Digipeat",
	"    Callsign Validation",
	"    MHeard list length  ( 0=disabled )", 
	"    Digipeat up / downlink control", 0
};

static char *level3text[] = 
{	"Select a parameter from below or hit Escape for previous menu",
	"    Node alias",
	"    Size of destination node table",
	"    Minimum auto update quality",
	"    HDLC ( radio port ) default quality",
	"    RS232( crosslink  ) default quality",
	"    Initial obsolescence count",
	"    Minimum obsolescence count for broadcast",
	"    Nodes broadcast interval ( seconds )",
	"    Initial 'time to live' ( hops )",
	"    Selective nodes broadcasts on ports",
	"    Crosslink port node b'cast interval (s)",
	"    Node broadcast algorithm port control", 
	"    Hash node broadcast port control",
	"    Control of Slime Trails", 0
};

static char *level4text[] = 
{	"Select a parameter from below or hit Escape for previous menu",
	"    Transport FRACK timeout ( seconds )",
	"    Transport retry counter",
	"    Transport acknowledge delay ( seconds )",
	"    Transport busy delay ( seconds )",
	"    Transport window size",
	"    Transport overfill limit ( frames )", 0 
};

static char *level7text[] = 
{	"Select a parameter from below or hit Escape for previous menu",
	"    No activity timeout ( seconds )",
	"    Beacon mode control",
	"    Beacon period ( seconds )",
	"    Beacon digipeat callsign",
	"    CQ enable flag",
	"    Information message",
	"    Password",
	"    Connect redirector",
	"    User message control flags",
	"    Auto reconnect to node", 0
};

static char *metertext[] = 
{	"Select a parameter from below or hit Escape for previous menu",
	"    Meter Control Flags Menu",
	"    Deviation Meter Scale Factor",
	"    S Meter Noise Floor ADC reading",
	"    S Meter Scale Factor",
	"    dBm Meter Scale Factor",
	"    dBm Noise Floor",
	"    Voltmeter Channel 1 Scale Factor",
	"    Voltmeter Channel 2 Scale Factor", 
	"    Voltmeter Channel 1 Offset Value",
	"    Voltmeter Channel 2 Offset Value", 0
};

static char *misctext[] = 
{	"Select a parameter from below or hit Escape for previous menu",
	"    Host command lead-in char ( Escape )", 0
};

static char *meterflags_menu_help[] = 
{	"This menu allows configuration of the meter control flags,",
	"namely the following :",
	"    En/Disabling of the deviation meter",
	"    En/Disabling of the signal strength meter",
	"    Whether signal strength is in 'Sn' or 'dBm'",
	"    En/Disabling voltmeter channel 1",
	"    En/Disabling voltmeter channel 2", 0
};

static char *level1_menu_help[] = 
{	"This menu allows configuration of the level 1 parameters,",
	"namely the following :",
	"    Persistance",
	"    Slot time",
	"    Full duplex flag",
	"    Send flags if no data needed",
	"    Transmit keyup delay",
	"    CWID repeat period",
	"    CWID keyer speed",
	"    RS232 host mode operation",
	"    RS232 crosslink protocol", 0 
};

static char *level2_menu_help[] = 
{	"This menu allows configuration of the level 2 parameters,",
	"namely the following :",
	"    Node callsign",
	"    FRACK ( T1 ) time",
	"    AX.25 window size ( MAXFRAME )",
	"    AX.25 retries",
	"    Ack ( T2 ) time",
	"    Active check ( T3 ) time",
	"    Digipeat enable / disable",
	"    Callsign Validation enable / disable",
	"    MHeard list length & enable / disable", 
	"    Control of L2 digi up & downlinks", 0
};

static char *level3_menu_help[] = 
{	"This menu allows configuration of the level 3 parameters,",
	"namely the following :",
	"    Node alias",
	"    Size of destination node table",
	"    Minimum auto update quality",
	"    HDLC ( radio port ) default quality",
	"    RS232( crosslink  ) default quality",
	"    Initial obsolescence count",
	"    Minimum obsolescence count for broadcast",
	"    Nodes broadcast interval",
	"    Initial 'time to live' for network frames",
	"    Selective nodes broadcasts on ports",
	"    Crosslink port node broadcast interval",
	"    Node broadcast algorithm on ports",
	"    Broadcast of hash nodes on ports",
	"    Control of Slime Trails", 0 
};

static char *level4_menu_help[] = 
{	"This menu allows configuration of the level 4 parameters,",
	"namely the following :",
	"    Transport FRACK timeout",
	"    Transport retry counter",
	"    Transport acknowledge delay",
	"    Transport busy delay",
	"    Transport window size",
	"    Transport overfill limit ( frames per connection )", 0 
};

static char *level7_menu_help[] = 
{	"This menu allows configuration of the level 7 ( PAD ) parameters,",
	"namely the following :",
	"    No activity timeout",
	"    Beacon mode control",
	"    Beacon period control",
	"    Beacon digi",
	"    CQ enable flag",
	"    Information message",
	"    Password",
	"    Connect redirector",
	"    Enable / disable of user messages",
	"    Enable / disable auto node reconnect", 0
};

static char *misc_menu_help[] = 
{	"This menu allows configuration of miscellaneous parameters,",
	"namely the following :",
	"    Host terminal command lead-in char ( escape )", 0 
};

static char *persistance_help[] =
{	"Persistance is the method by which the radio channel is shared",
	"between stations. It is an exponential decay means by which",
	"the channel is seized. It takes a value between 1 and 255.",
	"The probability that the channel will be seized at each interval",
	"of SLOTTIME, is (persistance / 256 ).", 0
};

static char *slottime_help[] =
{	"Slottime is the delay between intervals when waiting to transmit.",
	"Every interval of Slottime, the node generates a new random",
	"number for persistance checking. If the decision is not to seize",
	"the channel, the node waits another period of slottime.",
	"The units of slottime are tens of milliseconds.", 0
};

static char *fullduplex_help[] =
{	"The full duplex flag should only be set if the radio",
	"is capable of operating full duplex.",
	"What happens normally is that the node waits until DCD",
	"goes off before seizing the channel.",
	"When the full duplex flag is set, it dispenses with",
	"the nicety of checking for a free channel - it just",
	"keys up the transmitter.", 0
};

static char *sendflags_help[] =
{	"In a full duplex system, it can be convenient to",
	"send acknowledgements to packets even if there is",
	"no data waiting, or to acknowledge all packets.",
	"Setting this flag enables this ( I think ! ).", 0
};

static char *txdelay_help[] =
{	"This parameter sets the transmitter keyup delay.",
	"This should be long enough to cope with the TX",
	"transmit delay, the worst case user's squelch",
	"open delay and the time for their receiver to",
	"lock onto the data.",
	"There is no simple recommended value !", 0
};

static char *cwidperiod_help[] =
{	"The CWID repeat period is the time delay between the",
	"CW idents. If it is set to zero, CWID is disabled.",
	"DO NOT set this too short - just enough to obey your",
	"license.",
	"The message sent by CW is the node's callsign ( wthout",
	"the SSID ).", 0
};

static char *cwidspeed_help[] =
{	"This parameter sets the speed of sending the CWID.",
	"It is in units of tens of milliseconds. The speed",
	"in words per minute is 120/n, where n is the value",
	"of this parameter. Hence a setting of 6 corresponds",
	"to 20 wpm.", 0
};

static char *hostmode_help[] =
{	"This parameter sets the mode of operation of the host",
	"port ( ie the RS232 port when pin 23 is NOT tied low",
	"for crosslink. When set to 0 ( ie disabled, ie standard",
	"mode ), the node acts as a standard node with terminal",
	"connected. When set ( ie DCD node ), the escape-C and",
	"escape-D commands are disabled. Instead, the node uses",
	"hardware handshaking to connect & disconnect", 0
};

static char *crosslink_help[] =
{	"This parameter sets the protocol used on the RS232",
	"port, when pin 23 is tied low. When set to 0 ( ie",
	"the standard mode ), the standard TheNet crosslink",
	"format is used. The other settings replace the",
	"crosslink protocol with KISS.",
	"There are 3 variants of this. The simplest mode just",
	"replaces crosslink with KISS. In the next version,",
	"packets received by the node that are not intended for",
	"it, as well as UI frames, are copied to the other port",
	"to the one on which it was received. In this mode, a",
	"KISS application such as TCP/IP and the node share the",
	"same TNC & radio. In the final mode, ALL packets received",
	"on one port are copied to the other. This is useful in",
	"debugging a link or running something like AXSTAT",
	"Note packets sent BY the node are never copied across", 0
};

static char *nodecall_help[] =
{	"This sets the node's AX.25 callsign. It is not checked",
	"for validity, but must consist of alphabetic & numeric",
	"characters ( 1 to 6 ), followed optionally by a hyphen",
	"and an SSID in the range 0-15.",
	"Any callsign entered is converted to uppercase.", 0
};

static char *T1_help[] =
{	"This is the AX.25 link FRACK parameter.",
	"It is the number of seconds between retries",
	"for unacknowledged packets, in seconds", 0
};

static char *L2window_help[] =
{	"This is the maximum number of packets that may be sent",
	"on a level 2 link without acknowledgement. After this",
	"the node needs acknowledgement before proceeding with",
	"other frames", 0
};

static char *L2retry_help[] =
{	"This is the number of retries made at level 2, either",
	"to establish a link or in polling for acknowledgement.",
	"If set to zero, it will try forever!.", 0
};

static char *T2_help[] =
{	"This is the length of time the node waits after receiving",
	"a packet before sending an RR/RNR/REJ packet. The idea is",
	"to try to optimise use of the channel in piggy-backing the",
	"response on another packet. The parameter is in units of",
	"tens of milliseconds.", 0
};

static char *T3_help[] =
{	"This is the AX.25 T3 timer initialiser. On an inactive",
	"link, the node will poll the other station after this",
	"length of time to check if it is still there and to",
	"try to remove deadlocks. it is in units of tens of",
	"milliseconds.", 0
};

static char *digipeat_help[] =
{	"This flag, when enabled, allows other stations",
	"to digipeat ( ie L2 relay ) through the node.", 0
};

static char *callvalidate_help[] =
{	"This flag, if set, will only allow calls to be made",
	"to stations where a 'valid' callsign is entered or",
	"where the call is a transport connection. It is",
	"used by the level 7 switch.", 0
};

static char *mheard_help[] =
{	"This parameter sets the heard list size. It may also",
	"be used to disable the updating of the list by setting",
	"it to zero.",
	"Each entry in the list takes a whole buffer, so keep it",
	"as small as possible", 0
};

static char *alias_help[] =
{	"This is the node's network layer node name ( alias )",
	"It is 1-6 characters long, and must consist of",
	"letters, numbers or the '#' character. It is",
	"converted to uppercase.", 0
};

static char *nodelistsize_help[] =
{	"This parameter sets the size of the node destination",
	"table. Each entry can hold details about a single",
	"node complete with its 3 neighbour details.",
	"Each entry takes a buffer, so keep it as small as",
	"possible.", 0
};

static char *autoupdate_help[] =
{	"This is the minimum quality for a remote node to",
	"possess in order that it get included in the node's",
	"destination lists. If set to zero, then auto update",
	"of the destination list is disabled.", 0
};

static char *HDLCquality_help[] =
{	"This is the default quality applied to all nodes heard",
	"that do not possess an explicit entry in the table ( ie",
	"one made manually by the sysop ). This parameter is for",
	"nodes heard on the HDLC ( radio ) port.", 0
};

static char *RS232quality_help[] =
{	"This is the default quality applied to all nodes heard",
	"that do not possess an explicit entry in the table ( ie",
	"one made manually by the sysop ). This parameter is for",
	"nodes heard on the RS232 ( crosslink ) port.", 0
};

static char *obsolescence_help[] =
{	"This is the initial value of obsolescence applied to",
	"a node when a node broadcast is heard. It is then",
	"decremented each time a nodes broadcast is sent.", 0
};

static char *minobsolescence_help[] =
{	"This is the minimum value of obsolescence that an",
	"entry in the destination list must possess in order",
	"that it get included in the nodes broadcasts. It",
	"should be at least 1 lower than the obsolescence",
	"parameter.", 0
};

static char *bcastinterval_help[] =
{	"This is the period between node broadcasts made",
	"on the radio port. If the crosslink nodes broadcast",
	"interval is zero, it is also the period of node",
	"broadcasts on the crosslink port", 0
};

static char *TTL_help[] =
{	"This is the initial time to live parameter for level 4",
	"connections started from this node. It is decremented",
	"each time a frame is relayed by a level 3 node.", 0 
};

static char *selportbcast_help[] =
{	"This parameter controls which port nodes broadcasts",
	"are made on. Broadcasts may be made selectively on",
	"any combination of the 2 ports. It should usually",
	"be set to broadcast on both ports.", 0
};

static char *crossbcast_help[] =
{	"This controls the frequency of nodes broadcasts on",
	"the RS232 ( crosslink ) port. If set to zero, then",
	"nodes broadcasts are sent at the basic node broadcast",
	"interval. Otherwise, they are sent at this rate. Note",
	"that decrementing of the obsolescence counter happens",
	"at the rate of the main parameter, not this parameter.", 0
};

static char *nodealgo_help[] =
{	"This parameter sets the algorithm used for updating",
	"of the destination lists. When set, the algorithm is",
	"extended to include the condition :",
	"    do not broadcast a route to a node on port x if the",
	"    best quality neighbour was on the same port.",
	"The objective is to stop multiple node systems from",
	"getting their nodes tables clogged with spurious routes.",
	"It makes little sense to set it for the radio port, so ",
	"in general it will be set only on the crosslink port if",
	"at all.", 0
};

static char *hashnode_help[] =
{	"This parameter allows control over whether nodes with",
	"aliases that start with a '#' get broadcast in the",
	"nodes broadcasts. This is to allow nodes with a '#'",
	"to be prevented from being propagated. The main use",
	"is to prevent broadcasts on the radio port whilst",
	"still propagating them on the crosslink port.", 0
};

static char *L4frack_help[] =
{	"This parameter controls the transport retry timer.",
	"It should be quite long as in general there should",
	"not be retries at level 4 - that is level 2's job!",
	"It usually means a node is out temporarily ( or is",
	"dead ! ).", 0
};

static char *L4retry_help[] =
{	"This sets the number of level 4 retries. It should be",
	"quite small as in general there should not be a need",
	"for retries, that is handled by level 2!. It may mean",
	"that a node is out of service.", 0
};

static char *L4ack_help[] =
{	"This is the delay after receiving a frame of data before",
	"an acknowledgement is sent. It is designed to allow an",
	"ACK to piggyback on data ( if available ).", 0
};

static char *L4busy_help[] =
{	"This is the release time from a choked state.",
	"If the node is sent a choke packet, it should also get",
	"a release. If it does not, then it will release anyway",
	"after this time.", 0
};

static char *L4window_help[] =
{	"This controls the size of the transmit & receive windows.",
	"On transmit, it is the number of frames that can be sent",
	"without an acknowledgement. On receive, it is the number",
	"of frames that can be held out of sequence whilst",
	"awaiting a selective resend of an earlier packet.",
	"A big number will gobble up buffers nicely!.", 0
};

static char *overfill_help[] =
{	"This is the transport layer receive overfill limit.",
	"It specifies the maximum number of transport frames",
	"that can be stored in the transport layer before a",
	"choke frame is sent to the sending node.", 0
};

static char *noactivity_help[] =
{	"This is the no activity timer.",
	"If a station connected to the switch does nothing for",
	"this time ( more specifically, there is no data on",
	"that channel ) for this period of time, they will be",
	"disconnected.", 0
};

static char *beaconmode_help[] =
{	"This sets the mode of ID beacons.",
	"Bacon messages may be enabled, disabled or set to occur",
	"only when there is activity on the radio channel", 0 
};

static char *beaconperiod_help[] =
{	"This parameter sets the interval between beacon",
	"messages. In the original TheNet these were every",
	"10 minutes ( 600 seconds ). By this parameter, the",
	"period may be changed according to prevailing license",
	"conditions", 0
};

static char *beacondigi_help[] =
{	"In certain ( RARE ! ) situations, it is desirable to",
	"be able to send ID ( ie beacon ) messages via another",
	"station by digipeating. This parameter sets the callsign",
	"of the station to digipeat through. The callsign may",
	"also be cleared to disable digipeating the beacon message.", 0
};

static char *cqenable_help[] =
{	"The packet switch CQ command may be disabled in 2 different",
	"ways. Firstly, by the use of the 'CQ -' sysop command, and",
	"secondly by the use of this parameter. ( must get rid of this",
	"one one of these days ).", 0
};

static char *infomsg_help[] =
{	"This is the station info message. It is the first half of",
	"the info message sent when a user gives the 'Info' command,",
	"the other half being the sysop set info message.",
	"Unlike the sysop message, this message may contain carrage",
	"return characters to spread the message over a number of lines.", 0
};

static char *password_help[] =
{	"This parameter sets the eprom ( default ) password.",
	"The password should ideally consist of a random",
	"selection of characters chosen from the full ascii",
	"character set, and be of the full length of 80 characters.",
	"The program will choose a pseudorandom password for you if",
	"you request it.", 0
};

static char *conredir_help[] =
{	"If a user gives the 'Connect' message but does not specify",
	"a destination callsign, the original version of TheNet",
	"attempted to connect to the host via the RS232 port.",
	"Unless there was such a host, this got nowhere. In this",
	"version, such connect requests are redirected to either the",
	"BBS, DXCluster or HOST destinations ( assuming such are set ! ).", 0
};

static char *usermessage_help[] =
{	"These are boolean flags that allow individual user switch",
	"help messages to be enabled or disabled.", 0
};

static char *escape_help[] =
{	"This parameter is the command lead-in character.",
	"This is the character that is recognised as",
	"signifying the start of a command when using",
	"the host port and a terminal. Typically, this",
	"would be the escape character, so to connect to",
	"the node, the host terminal user would hit the",
	"escape key followed by 'C'. The character may be",
	"redefined using this parameter. The decimal value",
	"of the desired ASCII character should be entered.", 0
};

static char *save_help[] =
{	"This command allows the current configuration to be saved",
	"back to the file so that it may be burnt into an eprom.",
	"The old version of the file will be deleted.", 0
};

static char *ip_addr_help[] =
{
	"This allows the input of an IP style address.",
	"The format is",
	" ",
	"    nnn.nnn.nnn.nnn",
	" ",
	"where nnn is an integer in the range 0..255",
	"Note that it may be 1,2 or 3 characters long",
	"(ie leading zeros are not needed)",
	"and amateur addresses start '44'",
	"My address is therefore 44.131.16.31 and",
	"my broadcast address here in the uk is 44.131.0.0",
	NULL
};

static char *ip_TTL_help[] =
{
	"This is the default Time to Live for IP frames that",
	"originate from this node. It may take any value up",
	"to 255, and a typical value is about 20 ish",
	NULL
};

static char *ip_ports_help[] =
{
	"This sets the default mode of operation on each of the",
	"ports on the TNC. Each may be set to Datagram mode (DG)",
	"or may be set to Virtual Circuit mode (VC).",
	"The default mode is the one used if nothing else",
	"sets the mode ( like the IP frame flags or the ARP entry",
	NULL
};

static char *ip_enable_help[] =
{
	"This allows the operation of the IP router to be",
	"enabled or disabled.",
	NULL
};

static char *trying_help[] =
{
	"This message is sent to the user when the BBS, DXCluster",
	"or HOST commands are used. They tell the user that",
	"something is happening such as",
	"   Trying to connect to G9XYZ...",
	NULL
};

static char *sysopall_help[] =
{
	"This item causes the sysop ( or manager ) who has",
	"passed authentication to be shown ALL commands when",
	"an invalid command is entered",
	NULL
};

static char *goodbye_help[] =
{
	"This flag causes the node to say 'goodbye' to a",
	"used who enters the Bye or Quit commands",
	NULL
};

static char *connect_help[] =
{
	"When enabled, and when a Ctext message is set, then a",
	"user who uplinks to the node alias gets a welcome",
	"message",
	NULL
};

static char *talk8bit_help[] =
{
	"When enabled, then user data in TALK sessions is",
	"passed as 8 bit data rather than having the top",
	"bit forced to zero.",
	NULL
};

static char *nodelcase_help[] =
{
	"When enabled, then any instance of checking a node",
	"alias within the switch ( i.e. connect request to",
	"a node's alias or sysop node forcing ) is made case",
	"sensitive. BEWARE - this could confuse users !",
	NULL
};

static char *store_help[] =
{
	"This command allows the current set of parameters",
	"to be stored in a disk file in a manner that",
	"permits them to be editied or loaded again.",
	"The idea is that either a full set or a partial",
	"set may be reloaded to configure some or all of",
	"a new rom image.",
	NULL
};

static char *load_help[] =
{
	"This command allows the a set of parameters",
	"that had been stored in a disk file to be loaded.",
	"The idea is that either a full set or a partial",
	"set may be reloaded to configure some or all of",
	"a new rom image.",
	NULL
};

static char *dispalias_help[] =
{
	"This flag determines whether, when the user issues",
	"the 'routes' command, the neighbours are shown as",
	"'callsign', or ( if enabled ) as 'alias:callsign'.",
	NULL
};

static char *reconnect_help[] =
{
	"If set, this parameter causes the node to",
	"reconnect users who connect to it, then",
	"connect on to another node and get disconnected",
	"from it.",
	NULL
};

static char *digiupdown_help[] =
{
	"This parameter controls whether the node will",
	"allow digipeat uplinks to the node, or digipeat",
	"downlinks from the node. This differs from the",
	"digipeat control word in the following way. The",
	"digipeat control word is used to control whether",
	"the node will act as a digipeater. This parameter",
	"controls whether the node will respond when a link",
	"to or from it has been or will be digipeated to",
	"reach the node. Each bit controls a function as",
	"shown below. When a bit is set, connections of that",
	"type will be prohibited",
	"Bit 0 - bar digipeated connections to the node",
	"Bit 1 - bar digipeated downlinks from the node",
	NULL
};

static char *noslime_help[] =
{
	"This parameter controls 'slime trails'.",
	"If set to zero, the node operates as normal. If set to",
	"another value, it operates as shown below",
	"    1    Do not display slime trails in node lists",
	"    2    Do not accept slime trails",
	"    3    Neither accept or display slime trails",
	" ",
	"Not accepting slime trails will cause the node to",
	"ignore circuits from stations unless they are the",
	"subject of an acceptable node broadcast. Do not use",
	"this unless you understand the implications !",
	NULL
};

static char *devmeter_help[] =
{
	"This parameter sets the dev meter scaling factor.",
	"Please consult the documentation to decide on the",
	"correct setting",
	NULL
};

static char *smeternoise_help[] =
{
	"This parameter sets the S meter noise floor.",
	"This is the value that is subtracted from the",
	"ADC to get a zero referenced reading for the signal.",
	"Please consult the documentation to decide on the",
	"correct setting",
	NULL
};

static char *smetermult_help[] =
{
	"This parameter sets the S meter multiplier.",
	"This is the value that ADC readings are multiplied",
	"by to get a reading of the form 'S1...S9+'.",
	"Please consult the documentation to decide on the",
	"correct setting",
	NULL
};

static char *dbmetermult_help[] =
{
	"This parameter sets the dBm meter multiplier.",
	"This is the value that ADC readings are multiplied",
	"by to get a reading of the form '-nnn dBm'.",
	"Please consult the documentation to decide on the",
	"correct setting",
	NULL
};

static char *dbmeterfloor_help[] =
{
	"This parameter sets the dBm meter noise floor.",
	"This is the value that ADC readings offset by",
	"to get a reading of the form '-nnn dBm'.",
	"Please consult the documentation to decide on the",
	"correct setting",
	NULL
};

static char *voltmetermult_help[] =
{
	"This parameter sets a voltmeter channel multiplier.",
	"This is the value that ADC readings are multiplied",
	"by to get a reading of the form 'nn.n V'.",
	"Please consult the documentation to decide on the",
	"correct setting",
	NULL
};

static char *voltmeterofs_help[] =
{
	"This parameter sets a voltmeter channel offset.",
	"This is the value that is subtracted from the ADC",
	"readings before applying the multiplier factor.",
	"Please consult the documentation to decide on the",
	"correct setting.",
	NULL
};

static char *texnet_help[] =
{
	"This flag controls the TexNet interface handler.",
	"If set, then TexNet '*** LINKED to' messages will",
	"cause callsign mapping to occur.",
	"Please consult the documentation for details.",
	NULL
};

static char *devmeterenable_help[] =
{
	"This flag controls whether the heard list will",
	"show received signal deviation readings.",
	NULL
};

static char *smeterenable_help[] =
{
	"This flag controls whether the heard list will",
	"show received signal strength readings.",
	NULL
};

static char *smeterass_help[] =
{
	"This flag controls whether the heard list will",
	"show received signal strength readings as",
	"'S1..S9' ( if on ) or 'dBm' ( if off ).",
	NULL
};

static char *voltmeter_help[] =
{
	"This flag controls whether one of the",
	"voltmeter channels will be displayed.",
	"when a user issues the 'ADC' command.",
	NULL
};

static char *voltresolution_help[] =
{
	"This flag controls whether one of the",
	"voltmeter channels will be displayed",
	"as an integer or with an extra digit",
	"of resolution ( e.g. 23.4 as opposed",
	"to 23 ).",
	NULL
};

static char *voltdivide_help[] =
{
	"This flag controls whether the voltmeter",
	"channel intermediate value will be",
	"divided by 1000 or 100. If 'enabled', then",
	"the value used will be 1000.",
	"For more information, consult the documentation.",
	NULL
};

static MENULINE level1_menu_text[] = 
{
	{ 2,	&level1text[1], 0x66, persistance_help, 64, d_int, e_int, 0, 255 },
	{ 2,	&level1text[2], 0x68, slottime_help, 10, d_int, e_int, 0, 127 },
	{ 3,	&level1text[3], 0x7c, fullduplex_help, 0, d_bool, e_int, 0, 1 },
	{ 3,	&level1text[4], 0x7d, sendflags_help, 0, d_bool, e_int, 0, 1 },
	{ 1,	&level1text[5], 0x7f, txdelay_help, 30, d_int, e_int, 0, 255 },
	{ 2,	&level1text[6], 0x122, cwidperiod_help, 1800, d_int, e_int, 0, 3600 },
	{ 1,	&level1text[7], 0x124, cwidspeed_help, 6, d_int, e_int, 4, 10 },
	{ 5,	&level1text[8], 0x125, hostmode_help, 0, d_host_mode, e_int, 0, 1 },
	{ 6,	&level1text[9], 0x126, crosslink_help, 0, d_crosslink, e_int, 0, 3 }
};

MENU level1_menu =
{
	level1_menu_text,
	level1text,
	"Level 1 parameters",
	level1_menu_help
};

static MENULINE level2_menu_text[] = 
{
	{ 10,	&level2text[1], 0x3b, nodecall_help, 0, d_callsign, e_callsign, 0, 0 },
	{ 2,	&level2text[2], 0x6a, T1_help, 5, d_int, e_int, 1, 15 },
	{ 2,	&level2text[3], 0x6c, L2window_help, 2, d_int, e_int, 1, 7 },
	{ 2,	&level2text[4], 0x6e, L2retry_help, 10, d_int, e_int, 0, 127 },
	{ 2,	&level2text[5], 0x70, T2_help, 100, d_int, e_int, 0, 6000 },
	{ 2,	&level2text[6], 0x72, T3_help, 18000, d_int, e_int, 0, 65535 },
	{ 4,	&level2text[7], 0x74, digipeat_help, 0, d_bool, e_int, 0, 1 },
	{ 4,	&level2text[8], 0x76, callvalidate_help, 1, d_bool, e_int, 0, 1 },
	{ 1,	&level2text[9], 0x127, mheard_help, 20, d_int, e_int, 0, 100 },
	{ 1,	&level2text[10],0x150, digiupdown_help, 0, d_int, e_int, 0, 3 },
};

MENU level2_menu =
{
	level2_menu_text,
	level2text,
	"Level 2 ( AX.25 ) parameters",
	level2_menu_help
};

static MENULINE level3_menu_text[] = 
{
	{ 9,	&level3text[1], 0x42, alias_help, 0, d_alias, e_alias, 0, 0 },
	{ 2,	&level3text[2], 0x48, nodelistsize_help, 50, d_int, e_int, 1, 400 },
	{ 2,	&level3text[3], 0x4a, autoupdate_help, 10, d_int, e_int, 0, 255 },
	{ 2,	&level3text[4], 0x4c, HDLCquality_help, 10, d_int, e_int, 0, 255 },
	{ 2,	&level3text[5], 0x4e, RS232quality_help, 255, d_int, e_int, 0, 255 },
	{ 2,	&level3text[6], 0x50, obsolescence_help, 7, d_int, e_int, 0, 255 },
	{ 2,	&level3text[7], 0x52, minobsolescence_help, 5, d_int, e_int, 1, 255 },
	{ 2,	&level3text[8], 0x54, bcastinterval_help, 1800, d_int, e_int, 0, 65535 },
	{ 2,	&level3text[9], 0x56, TTL_help, 10, d_int, e_int, 0, 255 },
	{ 11,	&level3text[10], 0x128, selportbcast_help, 3, d_ports, e_int, 0, 3 },
	{ 2,	&level3text[11], 0x129, crossbcast_help, 600, d_int, e_int, 0, 65535 },
	{ 11,	&level3text[12], 0x12b, nodealgo_help, 2, d_ports, e_int, 0, 3 },
	{ 11,   &level3text[13], 0x137, hashnode_help, 0, d_ports, e_int, 0, 3 },
	{ 1,	&level3text[14],0x14f, noslime_help, 0, d_int, e_int, 0, 3 },
};

MENU level3_menu =
{
	level3_menu_text,
	level3text,
	"Level 3 ( network ) parameters",
	level3_menu_help
};

static MENULINE level4_menu_text[] = 
{
	{ 2,	&level4text[1], 0x58, L4frack_help, 300, d_int, e_int, 5, 600 },
	{ 2,	&level4text[2], 0x5a, L4retry_help, 2, d_int, e_int, 1, 127 },
	{ 2,	&level4text[3], 0x5c, L4ack_help, 3, d_int, e_int, 1, 60 },
	{ 2,	&level4text[4], 0x5e, L4busy_help, 180, d_int, e_int, 1, 1000 },
	{ 2,	&level4text[5], 0x60, L4window_help, 4, d_int, e_int, 1, 127 },
	{ 2,	&level4text[6], 0x62, overfill_help, 4, d_int, e_int, 1, 127 },
};

MENU level4_menu =
{
	level4_menu_text,
	level4text,
	"Level 4 ( transport ) parameters",
	level4_menu_help
};

static char *message_text[] =
{
	"Select a parameter from below or press ESC for previous menu",
	"    Issue a message when trying BBS etc",
	"    Show all available commands to sysop",
	"    Say 'goodbye' nicely",
	"    Give connect text on uplink to alias",
	"    Show nodes in routes as alias:callsign",
	"    Pass 8 bit data in TALK command",
	"    Make node alias handling case sensitive",
	"    Enable/Disable TexNet interface handler",
	NULL
};

static char *meterflags_text[] =
{
	"Select a parameter from below or press ESC for previous menu",
	"    Enable Deviation meter",
	"    Enable Signal strength meter",
	"    Show Signals as 'S1..S9+'.",
	"    Enable Voltmeter channel 1",
	"    Enable Voltmeter channel 2",
	"    Divisor is 1000 not 100, channel 1",
	"    Divisor is 1000 not 100, channel 2",
	"    Resolution setting fine, channel 1",
	"    Resolution setting fine, channel 2",
	NULL
};

static char *message_menu_help[] =
{
	"This menu allows a range of user help messages",
	"to be enabled or disabled",
	"as well as enabling case sensitive node alias",
	"handling, passing 8 bit data in 'Talk' and",
	"enabling or disabling the TexNet interface.",
	NULL
};

static char *mtu_ip0_help[] =
{
	"This parameter sets the MTU for the Radio port",
	"(i.e. port '0'). IP frames, sent to the node for",
	"routing, will be fragmented if they are to be",
	"sent on the radio port and if they exceed the",
	"size set for this parameter.",
	"**WARNING** Big MTUs can seriously damage your node",
	NULL
};

static char *mtu_ip1_help[] =
{
	"This parameter sets the MTU for the RS232 port",
	"(i.e. port '1'). IP frames, sent to the node for",
	"routing, will be fragmented if they are to be",
	"sent on the crosslink port and if they exceed the",
	"size set for this parameter.",
	"**WARNING** Big MTUs can seriously damage your node",
	NULL
};

static char *mtu_ipn_help[] =
{
	"This parameter sets the MTU for the Net/Rom port",
	"IP frames, sent to the node for routing, will be",
	"fragmented if they are to be sent by Net/Rom and",
	"if they exceed the size set for this parameter.",
	"Note that 236 is the max for Net/Rom compatibility.",
	NULL
};

static char *mtu_i_max_help[] =
{
	"This sets the Level 2 receive process limit for",
	"the number of data bytes in a level 2 AX.25",
	"packet before a level 2 reject frame is sent.",
	"It is set to one higher than the limit you want",
	"so for 256 byte packets, set it to 257",
	NULL
};

static char *mtu_l2_max_help[] =
{
	"This sets the limit beyond which a level 2 frame",
	"gets discarded. This is to protect the node above",
	"all else. If a frame is bigger than this, it is",
	"trashed. It is set to permit a packet of the max",
	"number of data bytes expected plus the addressing",
	"and control information of AX.25 plus a full set",
	"of digipeat calls. It therefore is 72 higher than",
	"the max number of data bytes ( ie 328 for 256 byte",
	"packets).",
	NULL
};

static MENULINE message_menu_text[] =
{
	{ 17,	&message_text[1], 0x143, trying_help, 0, d_bit, e_bit, 0, 1 },
	{ 18,	&message_text[2], 0x143, sysopall_help, 0, d_bit, e_bit, 0, 1 },
	{ 19,	&message_text[3], 0x143, goodbye_help, 0, d_bit, e_bit, 0, 1 },
	{ 20,	&message_text[4], 0x143, connect_help, 0, d_bit, e_bit, 0, 1 },
	{ 21,	&message_text[5], 0x143, dispalias_help, 0, d_bit, e_bit, 0, 1 },
	{ 22,	&message_text[6], 0x143, talk8bit_help, 0, d_bit, e_bit, 0, 1 },
	{ 23,	&message_text[7], 0x143, nodelcase_help, 0, d_bit, e_bit, 0, 1 },
	{ 24,	&message_text[8], 0x143, texnet_help, 0, d_bit, e_bit, 0, 1 },
};

MENU message_menu =
{
	message_menu_text,
	message_text,
	"Level 7 ( switch ) help messages ",
	message_menu_help
};

static MENULINE meterflags_menu_text[] =
{
	{ 17,	&meterflags_text[1], 0x152, devmeterenable_help, 0, d_bit, e_bit, 0, 1 },
	{ 18,	&meterflags_text[2], 0x152, smeterenable_help, 0, d_bit, e_bit, 0, 1 },
	{ 19,	&meterflags_text[3], 0x152, smeterass_help, 0, d_bit, e_bit, 0, 1 },
	{ 20,	&meterflags_text[4], 0x152, voltmeter_help, 0, d_bit, e_bit, 0, 1 },
	{ 21,	&meterflags_text[5], 0x152, voltmeter_help, 0, d_bit, e_bit, 0, 1 },
	{ 22,	&meterflags_text[6], 0x152, voltdivide_help, 0, d_bit, e_bit, 0, 1 },
	{ 23,	&meterflags_text[7], 0x152, voltdivide_help, 0, d_bit, e_bit, 0, 1 },
	{ 24,	&meterflags_text[8], 0x152, voltresolution_help, 0, d_bit, e_bit, 0, 1 },
	{ 17,	&meterflags_text[9], 0x153, voltresolution_help, 0, d_bit, e_bit, 0, 1 },
};

MENU meterflags_menu =
{
	meterflags_menu_text,
	meterflags_text,
	"Meter control flags ",
	meterflags_menu_help
};

static MENULINE level7_menu_text[] = 
{
	{ 2,	&level7text[1], 0x64, noactivity_help, 900, d_int, e_int, 0, 65535 },
	{ 7,	&level7text[2], 0x78, beaconmode_help, 2, d_beacon, e_int, 0, 2 },
	{ 2,	&level7text[3], 0x134, beaconperiod_help, 900, d_int, e_int, 600, 3600 },
	{ 12,	&level7text[4], 0x12c, beacondigi_help, 0, d_callsign, e_callsign, 0, 0 },
	{ 4,	&level7text[5], 0x7a, cqenable_help, 1, d_bool, e_int, 0, 1 },
	{ 14,	&level7text[6], 0xd1, infomsg_help, 0, d_infomsg, e_infomsg, 0, 0 },
	{ 15,	&level7text[7], 0x80, password_help, 0, d_password, e_password, 0, 0 },
	{ 8,	&level7text[8], 0x136, conredir_help, 0, d_conredir, e_int, 0, 2 },
	{ 0,    &level7text[9], &message_menu, message_menu_help, 0, NULL, NULL, 0, 0 },
	{ 3,	&level7text[10],0x14e, reconnect_help, 1, d_bool, e_int, 0, 1 },
};

MENU level7_menu =
{
	level7_menu_text,
	level7text,
	"Level 7 ( command interpreter ) parameters",
	level7_menu_help
};

static MENULINE misc_menu_text[] = 
{
	{ 1,	&misctext[1], 0x7e, escape_help, 0x1b, d_int, e_int, 0, 127 },
};

MENU misc_menu =
{
	misc_menu_text,
	misctext,
	"Miscellaneous parameters",
	misc_menu_help
};


char *mtu_menu_help[] =
{
	"This menu allows the IP router MTUs to be configured.",
	"Specifically, it permits the MTU related parameters",
	"of the node to be configured to allow the node to",
	"accept larger frames. Note that the last 2 parameters",
	"must be set to a size large enough to cater for the",
	"individual port MTUs.", 0
};

static char *mtu_text[] =
{
	"Select a parameter from below or hit ESC for the previous menu",
	"    MTU for AX.25 Level 2 IP on Radio port",
	"    MTU for AX.25 Level 2 IP on RS232 port",
	"    MTU for Net/Rom IP on L3 port",
	"    Max data bytes in a L2 packet",
	"    Total bytes in a L2 packet",
	NULL
};

static MENULINE mtu_menu_text[] =
{
	{  2,  &mtu_text[1], 0x144,  mtu_ip0_help,    256,  d_int,   e_int,  64,  1024 },
	{  2,  &mtu_text[2], 0x146,  mtu_ip1_help,    256,  d_int,   e_int,  64,  1024 },
	{  2,  &mtu_text[3], 0x148,  mtu_ipn_help,    236,  d_int,  e_int,  64,   236 },
	{  2,  &mtu_text[4], 0x14a,  mtu_i_max_help,   257,  d_int,  e_int, 257,  1025 },
	{  2,  &mtu_text[5], 0x14c,  mtu_l2_max_help,  328,  d_int,  e_int, 328,  1096 }
};

MENU mtu_menu =
{
	mtu_menu_text,
	mtu_text,
	"IP MTU Parameters",
	mtu_menu_help
};

char *ip_menu_help[] =
{
	"This menu allows the IP router to be configured.",
	"Specifically, the IP address may be set, the IP",
	"address used to denote broadcasts can be set,",
	"the default IP time-to-live may be set and the",
	"default mode of operation for each port may be",
	"defined. The IP router may also enabled or",
	"disabled.", 0
};

char *meter_menu_help[] =
{
	"This menu allows the ADC control parameters to be",
	"configured ( ie those associated with the 'meters').",
	"Specifically, the flags that control the meters,",
	"and the various multipliers and offsets that control",
	"the deviation meter, signal strength meter and",
	"voltmeters may be set.", 0
};

static char *ip_text[] =
{
	"Select a parameter from below or hit ESC for the previous menu",
	"    This node's IP ( Amprnet ) address",
	"    The IP ( Amprnet ) broadcast address",
	"    The default Time To Live ( TTL )",
	"    AX.25 ports mode control ( DG/VC )",
	"    Enable or Disable the IP router",
	"    Set MTU parameters menu",
	NULL
};

static MENULINE ip_menu_text[] =
{
	{ 16,  &ip_text[1], 0x138,  ip_addr_help,    0,  d_ip_addr, e_ipaddr,  0,  0 },
	{ 16,  &ip_text[2], 0x13c,  ip_addr_help,    0,  d_ip_addr, e_ipaddr,  0,  0 },
	{  1,  &ip_text[3], 0x141,  ip_TTL_help,    10,  d_int,     e_int,  2,255 },
	{ 27,  &ip_text[4], 0x140,  ip_ports_help,   0,  d_ip_ports,e_int,  0,  3 },
	{  3,  &ip_text[5], 0x142,  ip_enable_help,  1,  d_bool,    e_int,  0,  1 },
	{  0,  &ip_text[6], &mtu_menu, mtu_menu_help, 0, NULL, NULL, 0, 0 }
};

MENU ip_menu =
{
	ip_menu_text,
	ip_text,
	"IP Router Parameters",
	ip_menu_help
};

static MENULINE meter_menu_text[] =
{
	{ 0,	&metertext[1], &meterflags_menu, meterflags_menu_help, 0, NULL, NULL, 0, 0 },
	{ 1,	&metertext[2], 0x151, devmeter_help, 0, d_int, e_int, 0, 255 },
	{ 1,	&metertext[3], 0x154, smeternoise_help, 0, d_int, e_int, 0, 255 },
	{ 1,	&metertext[4], 0x155, smetermult_help, 0, d_int, e_int, 0, 255 },
	{ 1,	&metertext[5], 0x156, dbmetermult_help, 0, d_int, e_int, 0, 255 },
	{ 1,	&metertext[6], 0x157, dbmeterfloor_help, 0, d_int, e_int, 0, 255 },
	{ 1,	&metertext[7], 0x158, voltmetermult_help, 0, d_int, e_int, 0, 255 },
	{ 1,	&metertext[8], 0x159, voltmetermult_help, 0, d_int, e_int, 0, 255 },
	{ 1,	&metertext[9], 0x15a, voltmeterofs_help, 0, d_int, e_int, 0, 255 },
	{ 1,	&metertext[10], 0x15b, voltmeterofs_help, 0, d_int, e_int, 0, 255 },
};

MENU meter_menu =
{
	meter_menu_text,
	metertext,
	"ADC Meter Control Parameters",
	meter_menu_help
};

static MENULINE opening_menu_text[] = 
{
	{ 0,	&openingtext[1], &level1_menu, level1_menu_help, 0, NULL, NULL, 0, 0 },
	{ 0,	&openingtext[2], &level2_menu, level2_menu_help, 0, NULL, NULL, 0, 0 },
	{ 0,	&openingtext[3], &level3_menu, level3_menu_help, 0, NULL, NULL, 0, 0 },
	{ 0,	&openingtext[4], &level4_menu, level4_menu_help, 0, NULL, NULL, 0, 0 },
	{ 0,	&openingtext[5], &level7_menu, level7_menu_help, 0, NULL, NULL, 0, 0 },
	{ 0,	&openingtext[6], &ip_menu,     ip_menu_help,     0, NULL, NULL, 0, 0 },
	{ 0,	&openingtext[7], &misc_menu, misc_menu_help, 0, NULL, NULL, 0, 0 },
	{ 0,	&openingtext[8], &meter_menu, meter_menu_help, 0, NULL, NULL, 0, 0 },
	{ 13,	&openingtext[9], NULL, save_help, 0, NULL, NULL, 0, 0 },
	{ 25,	&openingtext[10], NULL, store_help, 0, NULL, NULL, 0, 0 },
	{ 26,	&openingtext[11], NULL, load_help, 0, NULL, NULL, 0, 0 }
};

char *opening_menu_help[] =
{
	"TheNet patcher for version X-1J Release 2", " ",
	"This program allows TheNet to be configured",
	"by means of a menu driven user interface.",
	"Help is available in a context sensitive manner",
	"on all menus ( well, almost all ! ).",
	"The program does not currently have version checking",
	"enabled, so it can be used for other versions of",
	"TheNet - BUT BE CAREFUL - because not all versions",
	"have the same parameters !!!!!",
	"The program looks for files called 'THENET1.X1J'",
	"and 'THENET2.X1J' in the current directory, but an",
	"alternative pair of files may be given as arguments",
	"to the program when invoked.", 0
};

MENU opening_menu =
{
	opening_menu_text,
	openingtext,
	"Main Menu",
	opening_menu_help
};

FILELINK filelink[] =
{
	{ "NodeCallsign",		&level2_menu_text[0] },
	{ "NodeAlias",			&level3_menu_text[0] },
	{ "NodeIpAddress",		&ip_menu_text[0] },
	{ "IpBroadcastAddress",		&ip_menu_text[1] },
	{ "TxKeyupDelay",		&level1_menu_text[4] },
	{ "Persistance",		&level1_menu_text[0] },
	{ "SlotTime",			&level1_menu_text[1] },
	{ "InformationMessage",		&level7_menu_text[5] },
	{ "Password",			&level7_menu_text[6] },
	{ "ConnectRedirector",		&level7_menu_text[7] },
	{ "CwidPeriod",			&level1_menu_text[5] },
	{ "CwidSpeed",			&level1_menu_text[6] },
	{ "FullDuplexFlag",		&level1_menu_text[2] },
	{ "FullDuplexSendFlags",	&level1_menu_text[3] },
	{ "HostMode",			&level1_menu_text[7] },
	{ "CrosslinkProtocol",		&level1_menu_text[8] },
	{ "L2WindowSize",		&level2_menu_text[2] },
	{ "L2RetryCounter",		&level2_menu_text[3] },
	{ "L2T1Timer",			&level2_menu_text[1] },
	{ "L2T2Timer",			&level2_menu_text[4] },
	{ "L2T3Timer",			&level2_menu_text[5] },
	{ "L2DigipeatFlag",		&level2_menu_text[6] },
	{ "CallValidateFlag",		&level2_menu_text[7] },
	{ "MHeardTableSize",		&level2_menu_text[8] },
	{ "NodeListSize",		&level3_menu_text[1] },
	{ "NodeAutoUpdate",		&level3_menu_text[2] },
	{ "HDLCPortQuality",		&level3_menu_text[3] },
	{ "RS232PortQuality",		&level3_menu_text[4] },
	{ "InitalObsolescence",		&level3_menu_text[5] },
	{ "MinimumObsolescence",	&level3_menu_text[6] },
	{ "BroadcastInterval",		&level3_menu_text[7] },
	{ "NetworkTimeToLive",		&level3_menu_text[8] },
	{ "SelectivePortBroadcast",	&level3_menu_text[9] },
	{ "CrosslinkBroadcastTime",	&level3_menu_text[10] },
	{ "NodeBroadcastAlgorithm",	&level3_menu_text[11] },
	{ "HashNodeBroadcastControl",	&level3_menu_text[12] },
	{ "TransportFrackTimer",	&level4_menu_text[0] },
	{ "TransportRetryCounter",	&level4_menu_text[1] },
	{ "TransportAckTimer",		&level4_menu_text[2] },
	{ "TransportBusyTimer",		&level4_menu_text[3] },
	{ "TransportWindowCount",	&level4_menu_text[4] },
	{ "TransportOverfillLimit",	&level4_menu_text[5] },
	{ "NoActivityTimer",		&level7_menu_text[0] },
	{ "BeaconMode",			&level7_menu_text[1] },
	{ "BeaconTimer",		&level7_menu_text[2] },
	{ "BeaconDigiCallsign",		&level7_menu_text[3] },
	{ "CQEnableFlag",		&level7_menu_text[4] },
	{ "HostEscapeCharacter",	&misc_menu_text[0] },
	{ "IpTimeToLive",		&ip_menu_text[2] },
	{ "IpPortModeFlags",		&ip_menu_text[3] },
	{ "IpEnableFlag",		&ip_menu_text[4] },
	{ "TryingToConnectMsgFlag",	&message_menu_text[0] },
	{ "SysopSeesAllCommandsFlag",	&message_menu_text[1] },
	{ "GoodbyeMessageFlag",		&message_menu_text[2] },
	{ "ConnectWelcomeMessageFlag",	&message_menu_text[3] },
	{ "ShowNodesAsAliasCallsign",	&message_menu_text[4] },
	{ "Pass8BitDataInTalkCommand",	&message_menu_text[5] },
	{ "MakeAliasesCaseSensitive",	&message_menu_text[6] },
	{ "IpMtuPort0",			&mtu_menu_text[0] },
	{ "IpMtuPort1",			&mtu_menu_text[1] },
	{ "IpMtuNetromPort",		&mtu_menu_text[2] },
	{ "IpMtuMaxL2InfoBytes",	&mtu_menu_text[3] },
	{ "IpMtuMaxTotalL2DataBytes",	&mtu_menu_text[4] },
	{ "AutoReconnectToNode",	&level7_menu_text[9] },
	{ "L2DigiUpAndDownlinkCtrl",	&level2_menu_text[9] },
	{ "DeviationMeterScale",	&meter_menu_text[1] },
	{ "SmeterNoiseFloor",		&meter_menu_text[2] },
	{ "SmeterScaleMultiplier",	&meter_menu_text[3] },
	{ "SmeterDbMultiplier",		&meter_menu_text[4] },
	{ "SmeterDbNoiseFloor",		&meter_menu_text[5] },
	{ "Voltmeter1Multipler",	&meter_menu_text[6] },
	{ "Voltmeter2Multipler",	&meter_menu_text[7] },
	{ "Voltmeter1Offset",		&meter_menu_text[8] },
	{ "Voltmeter2Offset",		&meter_menu_text[9] },
	{ "DevMeterEnableFlag",		&meterflags_menu_text[0] },
	{ "SMeterEnableFlag",		&meterflags_menu_text[1] },
	{ "SMeterAsSPointsFlag",	&meterflags_menu_text[2] },
	{ "VoltmeterChannel1Flag",	&meterflags_menu_text[3] },
	{ "VoltmeterChannel2Flag",	&meterflags_menu_text[4] },
	{ "VoltmeterChannel1Divisor",	&meterflags_menu_text[5] },
	{ "VoltmeterChannel2Divisor",	&meterflags_menu_text[6] },
	{ "VoltmeterChannel1Resolution",	&meterflags_menu_text[7] },
	{ "VoltmeterChannel2Resolution",	&meterflags_menu_text[8] }
};

unsigned filelinksize = sizeof( filelink ) / sizeof( FILELINK );
