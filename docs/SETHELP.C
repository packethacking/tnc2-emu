/**************************************************************************
 *
 *  SETHELP - a program that changes the help text in the X1J part 2 binary
 *
 * (c) By: John Bednar WB3ESS
 * This program is provided 'as is', with no warranty. It has been
 * produced for use in self-tuition in amateur radio and is not
 * to be used for commercial applications. It may be freely copied and
 * used under those conditions.
 *
 *
 * Invocation :
 *      SETHELP bin_infile bin_outfile new_textfile
 *      where: bin_infile   - the X1J binary file (part 2 only)
 *             bin_outfile  - the name of the binary file to be created
 *             new_textfile - the text file that contains the new help text
 *
 *     Create the text_file with your favorite editor and execute this
 *     program. It will tell you how many bytes are remaining or how
 *     many bytes too large your new help text is. If the starting
 *     address of the help text changes in a future version, a #define
 *     variable is provided to easily update this program.
 *
 *************************************************************************/

/*************************************************************************
 * Return codes:
 *
 * The program exits with an error code of 0 if all went correctly.
 * With 1 for a syntax error
 *      2 for duplicate binary file name error
 *      3 for an input file open error - original X1J file
 *      4 for an output file open error - new X1J file
 *      5 for an input file open error - help text file
 *      6 for an input file read error - original X1J file
 *      7 for an output file write error - new X1J file
 *      8 for an file read or write error - reading/writing the help text
 *      9 for a file that does not appear to be right version
 *************************************************************************/

/*************************************************************************
 *  Program revisions:
 *  version 1.0 - 1/20/94: mailed to K3RLI and N3DPU
 *  version 1.1 - 1/21/94: N3DPU found two variables that were not being
 *              used. I corrected that and added a test to the dumpX1code()
 *              to check the number of bytes that were written.
 *  version 1.2 - 2/14/93: G8KBB minor changes, updated for TheNet X-1J
 *              release 2 and renamed from HELPX1J. Added checksum routine
 *
 *************************************************************************/

#include <stdio.h>

#define UPTO 0x7c6c      /* help message starts with the next byte  */
#define ROMSIZE 32768	/* size of rom image segment space */
#define HELPSPACE (ROMSIZE-UPTO)  /* space at end of segment for help text */

unsigned char Buffer[ROMSIZE+8];    /* buffer for i/p data from file */
unsigned char *Ptr;             /* pointer into buffer for reading */
FILE *Fpin, *Fpin2, *Fpout;     /* input & output file handles */


main( argc, argv )
char *argv[];
{
        int ecode=0;            /* error code on exit from main loop */

        printf("\n  X-1J release 2 help text programmer by WB3ESS - Version 1.2\n");

        /* We need an input file, an output file, and an help text file.
         *  4 command line parameters are required.
         */
        if( argc != 4 ) 
        {
                printf( "Usage error, try:  sethelp bin_infile bin_outfile new_textfile\n");
                printf( "Where:  ");
                printf( "bin_infile   - the X1J binary file (part 2 only)\n");
                printf( "        bin_outfile  - the name of the binary file to be created\n");
                printf( "        new_textfile - the text file that contains the new help text\n");
                exit( 1 );
        }


        /*
         *  Check to see if the binary filenames are different
         */
        if(!strcmp(argv[1],argv[2])) 
        {
                printf("Error: binary filename names must be different.\n");
                exit( 2 );
        }


        /*
         * We seem to have the parameters - so let's open the input binary
         * file in binary mode.
         */
        if( ( Fpin = fopen( argv[1], "rb"  ) ) == NULL ) 
        {
                printf("Error: X1J binary file can not be opened.\n");
                exit( 3 );
        }

        /*
         *  Next, open the output binary file for write in binary mode.
         */
        if( ( Fpout = fopen( argv[2], "wb" ) ) == NULL ) 
        {
                printf("Error: binary file can not be opened.\n");
                exit( 4 );
        }

        /*
         *  Open the help text file for reading.
         */
        if( ( Fpin2 = fopen( argv[3], "rb"  ) ) == NULL ) 
        {
                printf("Error: text file can not be opened.\n");
                exit( 5 );
        }


        /*
         *  Now read the X1J binary file into memory
         */
        if( !readdata() )
        {
                printf("Error: X1J input file size error or read error.\n");
                exit( 6 );
        }


        /*
         *  Now check it is the right version
         */
        if( !checkdata() )
        {
                printf("Error: File does not appear to be THENET2.X1J.\n");
                exit( 9 );
        }


        /*
         *  Dump all the binary bytes up to (#UPTO) into the new file
         */
        if( !dumpX1code(UPTO)) 
        {
                printf("Error: can not write the binary data into a file.\n");
                ecode = 7;
        }

        /*
         *  Read the new help text & write into the binary file
         */
        if( !dumptxt() )
        {
                printf("Error: can not read help text or write into a file.\n");
                ecode = 8;
        }


        /*
         *  close files and exit
         */
        fclose( Fpin );
        fclose( Fpout );
        fclose( Fpin2 );
        return( ecode );
}




/**************************************************************************
 * This function reads the entire X1 data into memory.
 * A return of 0 means error, 1 means all went well.
 **************************************************************************/
readdata()
{
        unsigned int i;
        i = fread( Buffer, sizeof(char), sizeof(Buffer), Fpin );
        if( i < UPTO || i > ROMSIZE )
                return( 0 );

        if( ferror( Fpin ) )
                return( 0 );

        fclose( Fpin );
        return( 1 );
}



/***************************************************************************
 * This function dumps the binary portion of the X1 data into the new
 * binary file.
 * A return of 0 means error, a return of 1 means all went well.
 **************************************************************************/
dumpX1code( num )
unsigned int num;
{
        int i;
        i = fwrite( Buffer, sizeof(char), num, Fpout );
        if( i != num )
                return( 0 );
        if( ferror(Fpout) )
                return( 0 );
        return( 1 );
}



/***************************************************************************
 * This function reads the help text file, removes the '\n' characters,
 * and dumps them into the end of the new binary file.
 * It prints a message to the user telling how many bytes over or under
 * you are.
 * A return of 0 means error, a return of 1 means all went well.
 **************************************************************************/
dumptxt()
{
        unsigned char Textbuf[HELPSPACE+256];
        unsigned char *Ptrtxt;          /* pointer to the output text buffer */
        unsigned char *Ptrbuf;          /* pointer to the input text buffer */
        unsigned int i,txtsize;

        txtsize = fread( Buffer, sizeof(char), sizeof(Buffer), Fpin2 );
        if( ferror( Fpin2 ) )
                return( 0 );

        Ptrbuf = Buffer;        /*  set pointer to the beginning address */
        Ptrtxt = Textbuf;       /*  set pointer to the beginning address */

        for(i=0; i<txtsize; i++)
        {
                if ( *(Ptrbuf+i) != '\n' ) 
                {
                        *Ptrtxt = *(Ptrbuf+i);   /* copy if not a newline  */
                        Ptrtxt++;
                }
        }


        *Ptrtxt = '\0';         /*  terminate text file with a null  */
        Ptrtxt++;               /*  needed to get the proper size  */

        if(Ptrtxt-Textbuf > HELPSPACE)
        {
                printf("\n  ERROR: The binary output file is too large because the text file \n  contains too many characters.");
                printf(" You must remove %d byte(s) from the\n",(Ptrtxt-Textbuf)-HELPSPACE);
                printf("  input help text file and re-run this program.\n");
        }
        else
                printf("\n  You have %d byte(s) remaining for additional help text.\n", HELPSPACE-(Ptrtxt-Textbuf));

        fwrite( Textbuf, sizeof(char), (Ptrtxt-Textbuf), Fpout );
        if( ferror(Fpout) )
                return( 0 );
        return( 1 );
}

/***************************************************************************
 * This function does a rudimentary check for the right file version
 * A return of 0 means error, a return of 1 means all went well.
 **************************************************************************/
checkdata()
{
	unsigned *ptr;
	unsigned cksum = 0;
	
	for( ptr = Buffer+0x1080; ptr < Buffer+UPTO-2; ptr++ )
		cksum += *ptr;

#ifdef DEBUG
	fprintf( stderr, "Debug : checksum = 0x%04x\n", cksum );
#endif
	
	return( cksum == 0x8be3 );
}
