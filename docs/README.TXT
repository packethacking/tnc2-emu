This  disk  contains  the distribution  version  of  TheNet  X-1J 
release 2.

1. Hardware Requirements

     The software runs in a Z80 based TNC2 or similar clone  such 
     as the BSX2. It is installed as described in the  bankswitch 
     mods file, but essentially all it needs is a single piece of 
     wire  from pin 8 of the modem disconnect header to pin 1  of 
     the EPROM.
     
     The  EPROM  needed is a 27512, rather than the  27256  of  a 
     normal TNC2. Pin 1 of the EPROM is bent out from the  socket 
     and connected as described above.
     
     The TNC needs to be set to the higher clock speed of 4.9 MHz 
     as  a  minimum.  Whilst  this  version  of  TheNet  is  more 
     efficient  than  others, it is not sensible to  run  at  the 
     slower speed.
     
     The TNC needs at least 32K bytes of RAM.
     
     The  TNC needs to have DCDB from the SIO/0 connected to  pin 
     23 of the RS232 connector. See the section on RS232 later in 
     this note for more information.
     
2. Installing over TheNet X previous versions
     
     If  you are replacing a previous ROM with TheNet in  it,  be 
     sure to do a coldstart ( you may need to remove the  battery 
     link to force this ).
     
3. Files
        
     The files on this disk are :
        
          thenet1.x1j    Part 1 of the code
          thenet2.x1j    part 2 of the code
          readme.*       This file
          configur.*     Installation guide
          userguid.*     A user's guide to the node
          overview.*     The sysop's manual
          quickref.*     A handy quick reference guide
          bankswit.*     Info on the hardware bankswitching
          intro.*        The brief release note
          smeter.*       Details on the Smeter hardware
          devmeter.*     Details on the deviation meter
          patch.exe      A windowing driven patcher
          intel.exe      An Intel hex file dump utility
          intel.c        The source of the above
          motorola.exe   A Motorola S1 type file dumper
          motorola.c     The source of the above
          sethelp.exe    Utility to change the ROM help text 
          sethelp.c      The source of the above
          printpwd.exe   Utility to print a password nicely
          printpwd.c     The source of the above
          smeter.csv     Sample data for smeter calibration
          smeter.ljt     Circuit for an S meter in HP PCL format
          ipxlink.doc    Ideas on node IP addressing
        
     The files denoted '*' are stored in two formats
     
             .TXT ASCII versions of the files
             .DOC Word for Windows 2.0 format
        
     The  files  contain  the same except for  the  DEVMETER  and 
     SMETER,  as the ASCII version does not contain  the  circuit 
     diagrams  or graphs. The smeter circuit diagram is shown  in 
     laserjet  printer  format  (  i.e.  HPPCL  )  in  the   file 
     smeter.ljt. As this is a large file, it has been  compressed 
     into the zip file smeter.zip.
        
4. Using it with a TCP/IP system
        
     One of the reasons for the inclusion of the IP router was to 
     help  the  development  of  IP  networks.  This  is  in  two 
     different ways :
        
        
     It allows a IP station that does not run 24 hours to run  an 
     IP router for the others in the area without leaving the  PC 
     running,
        
         
     It allows existing nodes to double as IP routers.
        
     In scenario 1, where a station runs a TNC2 clone with a KISS 
     ROM  or  similar, this software may be used instead  of  the 
     KISS  ROM.  It should be configured to run  KISS,  Selective 
     Copy on the RS232 port.
        
5. The other version
        
     The  version that includes the HIGH and LOW commands is  not 
     included here. It used a form of bankswitching that  clashes 
     with the deviation meter.
        
6. Problems
        
     A  long standing problem with the software was its  tendency 
     to  crash  on  reset. I think I have now cured  it.  It  was 
     caused by an interaction between the bankswitching logic and 
     the  interrupt routines during reset. It should  not  happen 
     now - tell me if it does - but please be certain that it was 
     not  caused  by some other fault such as  wiring  errors  or 
     installing over a previous version without clearing the  RAM 
     first.  You should still take care to use fast enough  parts 
     for the clock rate.
     
     The biggest problem with TheNet X-1J was that some copies of 
     the  beta  test code are in circulation.  These  leaked  out 
     during testing and when I sent out a message telling  people 
     of  the  things to look for to identify  which  version  you 
     have,  some bright spark deleted the files that  showed  the 
     difference. Oh well. Live and learn.
     
     One  common problem. If your node appears to  'lose'  nodes, 
     look very carefully at the rates of node broadcasts and  the 
     setting  of the algorithm control. The RS232 rate should  be 
     faster than the radio rate or it should be set to zero,  and 
     the  alternative  algorithm should only be  enabled  on  the 
     RS232  port if at all. Try switching it off to see  if  that 
     helps.
     
     The  patcher  has been altered. The code now  comes  as  two 
     parts  and  the patcher patches both at the  same  time.  It 
     needs & expects to be able to access both parts.
        
7.   RS232 connections
        
     It is important that you understand the RS232 connections.
     
     The  RS232  port will operate in one of two basic  modes,  a 
     dumb terminal mode and a protocol mode. Each of these  modes 
     has two basic types of operation. These are detailed further 
     below.
     
     The  decision  as  to  whether the  node  operates  in  dumb 
     terminal or protocol mode depends on the setting of the DCDB 
     input  of the serial communications chip ( the SIO/0  ).  In 
     TNC2  clones,  this  is the signal on pin 23  of  the  RS232 
     connector, but in most TNCs you will need to either insert a 
     link  or  a piece of wire, as detailed in your  TNC  manual. 
     After  connecting the links if needed, then leaving  pin  23 
     open circuit will cause the TNC to operate in dumb  terminal 
     mode,  and setting it to a negative voltage ( ie  connecting 
     it to pin 10 ) will cause it to operate in protocol mode.
     
     If you use a BSX2 TNC, then you should connect pin 23 to pin 
     7  rather than pin 10 for protocol mode. You will also  need 
     to  insert a link into JMP14 & make sure there is no  jumper 
     in JMP9.
     
     If  you use one of the other common TNC2 clones such as  the 
     PK80,  MFJ1270, TNC-200 etc, then I understand ( but  cannot 
     confirm ) that you will need to connect a wire from JMP9 pin 
     1,2 or 3 - they are all joined on the PCB - to RS232 pin 23.
     
     Incidentally,  don't forget to increase the  watchdog  timer 
     capacitor  to give a 1 minute timeout ( C31 to 47uF in  TNC2 
     clones ).
     
     Finally, the protocol modes are CROSSLINK or KISS,  selected 
     by the firmware, and the dumb terminal modes are STANDARD or 
     MODEM,  again set by firmware. In the MODEM  mode,  hardware 
     handshaking  is used for connect request / disconnect.  This 
     is NOT the usual method of operation. Unless you are certain 
     you understand, DO NOT use this mode !.
     
8. Introduction to nodes
        
     If you have not used a TNC based node before, the  following 
     notes on the operation of the 'dumb terminal' port may be of 
     use.
     
     When  pin  23  on the RS232 interface is high,  and  with  a 
     terminal  or terminal emulator on the RS232 port,  the  node 
     will  not  appear  to respond to anything. If  you  send  an 
     'escape' character, the node should respond with a '* '.  If 
     it does not, look at the TNC status LED. It should be  dimly 
     lit with a little bit of a flicker. If not, the node is  not 
     operating.  If  it  is, you have an RS232  problem.  If  you 
     receive unsolicited garbage whenever the TNC receives  data, 
     you have pin 23 tied low and you are receiving crosslink  or 
     kiss frames.
     
     If it does respond with a star and a space, you can now type 
     one of 3 characters - C, D or P.
        
             P will display or set the current password.
             D will disconnect you from the node.
             C will connect you to the node.
     
     Try the C command. You should get a 'connected' message. Hit 
     '?'  followed  by  return and you will get  a  list  of  all 
     commands.
     
     Escape  commands  only work on the RS232 port when  in  dumb 
     terminal  mode.  Don't forget to disconnect  from  the  node 
     before you disconnect the terminal. 
        
9. 'User channel' nodes
        
     To  create  a  'user channel' node ( ie  one  that  supports 
     Net/Rom  protocols  on its RS232 port but not on  its  Radio 
     port ), do the following :
        
       
     * Set  the  radio  port default quality  to  one  less  than 
       minqual
     * Bar  node  broadcasts on the radio port  by  setting  MODE 
       parameter number four to 2
     * Ensure that no manual routes exist in the route table  for 
       port 0.
        
10. EADG Nodes
        
       If you put a node up in the UK that can be heard from East 
       Anglia, may I encourage you to put the QRA locator in  the 
       Beacon text to enable the G1TLH Beacon analysis program to 
       report accurately on propagation conditions.
        
        
73's
        
Dave G8KBB @ GB7MXM.#36.GBR.EU
[44.131.16.31] g8kbb@g8kbb.ampr.org
        
7, Rowanhayes Close
Ipswich
IP2 9SX
England

Tel    0473 682266
    +44 473 682266
